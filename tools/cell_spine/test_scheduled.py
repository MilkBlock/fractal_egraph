import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from . import scheduled, solver
from .compiler import ROOT


class ScheduledSpines(unittest.TestCase):
    def fixture(self):
        columns=lambda *s:[{'Slot':i} for i in s]
        atoms=[dict(table='A',sorts=['E','E','E'],columns=columns(0,1,2)),
               dict(table='Ctx',sorts=['E','E','E'],columns=columns(1,3,4)),
               dict(table='Ty',sorts=['E','E','E'],columns=columns(0,5,6)),
               dict(table='C',sorts=['E','E','E'],columns=columns(7,3,8))]
        shape=dict(slots=[[f'v{i}','E'] for i in range(9)],atoms=atoms,guards=[],native_residuals=False)
        plan=dict(shape=shape,seed=0,steps=[[1,{'Columns':1}],[2,{'Columns':1}],[3,{'Columns':2}]],prefix_len=0,carried_slots=[])
        return dict(ruleset='s',rule='q',shape=shape,plans=[plan])

    def test_full_spine_and_optional_inputs_are_not_two_atom_views(self):
        q=self.fixture();opts=list(scheduled.options(q,{'A':2},10))
        self.assertEqual({x['context_atoms'] for x in opts},{1,2,3,4})
        self.assertGreater(len({json.dumps(x['layout']['anchor']) for x in opts}),4)
        self.assertEqual({x['layout']['payload'] for x in opts},{'anchor','outside','all'})
        self.assertTrue(any(x['complete_prefix'] for x in opts))
        for x in opts:self.assertEqual(x['plan']['shape'],q['shape'])

    def test_joint_solver_counts_shared_index_once_and_off_option(self):
        solver.select('scip');q=self.fixture();q2=dict(q,rule='q2')
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp)
            plan=scheduled.select([q,q2],{'A':2},out,(1,1,1),
                {('s','q'):dict(candidate_rows=10000,requests=20),('s','q2'):dict(candidate_rows=0,requests=0)})
            self.assertEqual([x['rule'] for x in plan['selected']],['q'])
            self.assertGreater(plan['choices'],3)
            self.assertTrue((out/'scheduled-selection.lp').is_file())

    def test_complete_multi_ruleset_source_is_preserved_not_archived(self):
        source=ROOT/'experiments/cell_spine/fixtures/stages/multi_rulesets.egg'
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='scheduled-contract-') as temp:
            out=Path(temp)/'out'
            result=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',source,
                '--output',out,'--skip-build'],cwd=ROOT,capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            report=json.loads((out/'report.json').read_text())
            self.assertEqual(report['status'],'scheduled_symbolic');self.assertFalse(report['cellified'])
            self.assertEqual(source.read_bytes(),(out/'program.egg').read_bytes())
            self.assertFalse((out/'snapshot.egg').exists())
            plan=json.loads((out/'boundary-programs.json').read_text())
            self.assertEqual(plan['schema'],'cell-boundary-programs/v2')
            self.assertTrue(plan['rules'])

    def test_imported_analyze_certificate_reserves_its_queries(self):
        source=ROOT/'experiments/unified_ripsat_certificates/scalar-training.egg'
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='scheduled-certificate-') as temp:
            tmp=Path(temp);library=tmp/'certificates.egg';out=tmp/'compiled'
            bake=subprocess.run([ROOT/'target/analyzer/debug/egg_layout','analyze-transfer',source,library],cwd=ROOT,capture_output=True,text=True)
            self.assertEqual(bake.returncode,0,bake.stderr)
            result=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--mode','scheduled',
                '--source',source,'--output',out,'--certificate-library',library,'--skip-build'],cwd=ROOT,capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            proof=json.loads((out/'certificates.json').read_text())
            self.assertTrue(any(c['strength']=='guarded_closure' for c in proof['certificates']))
            self.assertTrue(proof['markers'])
            self.assertEqual((out/'program.egg').read_bytes(),source.read_bytes())
            self.assertEqual((out/'certificates.egg').read_bytes(),library.read_bytes())
            # Both numeric queries already belong to the checked library; the
            # solver cannot silently install a second executor for either one.
            self.assertFalse(json.loads((out/'boundary-programs.json').read_text())['rules'])

    def test_solver_pays_common_prefix_once_and_preserves_aliases(self):
        import copy
        from . import shared_prefix
        solver.select('scip');q=self.fixture();other=copy.deepcopy(q);other['rule']='q2'
        other['shape']['atoms'][-1]['table']='D'
        for p in other['plans']:p['shape']=other['shape']
        observed={}
        for o in scheduled.options(q,{'A':84},10):
            if o['plan']['prefix_len']<3:
                d=shared_prefix.describe(o['plan']);observed[d['body_key']]=dict(peak_bindings=84,lookups=100,candidate_rows=84,root_refreshes=100)
        profile=dict(markers=observed,queries={('s',r):dict(seed_evaluations=1000,emitted_bindings=1000) for r in ['q','q2']})
        with tempfile.TemporaryDirectory() as tmp:
            plan=scheduled.select([q,other],{'A':84,'Ctx':10000,'Ty':10000,'C':10000,'D':10000},Path(tmp),(1,1,1),marker_profile=profile)
            shared=[m for m in plan['shared_markers'] if len(m['consumers'])==2 and len(m['definition']['body']['atoms'])>1]
            self.assertTrue(shared,plan)
            self.assertTrue(all(q['plan']['shared_marker'] for q in plan['selected']))
            definition=shared_prefix.egg(plan['shared_markers']);self.assertIn('(relation SharedSpine',definition)
        base=copy.deepcopy(q['plans'][0]);base['prefix_len']=1;base['carried_slots']=list(range(9))
        rename=copy.deepcopy(base)
        for a in rename['shape']['atoms']:
            for c in a['columns']:
                if 'Slot' in c:c['Slot']=8-c['Slot']
        self.assertEqual(shared_prefix.describe(base)['key'],shared_prefix.describe(rename)['key'])
        changed=copy.deepcopy(base);changed['shape']['atoms'][1]['columns'][0]={'Slot':0}
        self.assertNotEqual(shared_prefix.describe(base)['key'],shared_prefix.describe(changed)['key'])


    def test_numeric_parameters_are_shared_without_merging_consumer_guards(self):
        import copy
        q=self.fixture();other=copy.deepcopy(q);other['rule']='q2'
        for query in [q,other]:
            query['shape']['slots'][3][1]='i64'
            for atom in query['shape']['atoms']:
                for i,col in enumerate(atom['columns']):
                    if col.get('Slot')==3:atom['sorts'][i]='i64'
            query['shape']['guards']=[[1,[{'Slot':3},{'Literal':'10'}]]]
            for plan in query['plans']:plan['shape']=query['shape']
        other['shape']['guards'][0][1][1]={'Literal':'20'}
        other['shape']['atoms'][-1]['table']='D'
        from . import shared_prefix
        measured={}
        for o in scheduled.options(q,{'A':84},10):
            if o['plan']['prefix_len']<3:
                d=shared_prefix.describe(o['plan']);measured[d['body_key']]=dict(peak_bindings=84,lookups=100,candidate_rows=84,root_refreshes=100)
        profile=dict(markers=measured,queries={('s',r):dict(seed_evaluations=1000,emitted_bindings=1000) for r in ['q','q2']})
        solver.select('scip')
        with tempfile.TemporaryDirectory() as tmp:
            r=scheduled.select([q,other],{'A':84,'Ctx':10000,'Ty':10000,'C':10000,'D':10000},Path(tmp),(1,1,1),marker_profile=profile)
        self.assertEqual(len(r['selected']),2)
        self.assertTrue(all(any(p['slot']==3 and p['stored'] for p in o['parameter_interface']['parameters']) for o in r['selected']))
        self.assertNotEqual(r['selected'][0]['parameter_interface']['guards'],r['selected'][1]['parameter_interface']['guards'])
        self.assertTrue(any(len(m['consumers'])==2 for m in r['shared_markers']))

    def test_analyze_numeric_queries_enter_solver_and_portable_marker_library(self):
        source=ROOT/'experiments/observed_transfer_queries/tuple-training.egg'
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='parametric-marker-') as temp:
            tmp=Path(temp);library=tmp/'certificates.egg';out=tmp/'compiled'
            bake=subprocess.run([ROOT/'target/analyzer/debug/egg_layout','analyze-transfer',source,library],cwd=ROOT,capture_output=True,text=True)
            self.assertEqual(bake.returncode,0,bake.stderr)
            result=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--mode','scheduled',
                '--source',source,'--output',out,'--certificate-library',library,'--skip-build'],cwd=ROOT,capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            manifest=json.loads((out/'scheduled-plan.json').read_text())
            chosen=manifest['selection']['selected']
            self.assertEqual(len(chosen),5)
            self.assertTrue(all(q['authority']=='analyze_complete_query' for q in chosen))
            native=json.loads((out/'work/queries.json').read_text())
            self.assertTrue(any(p['stored'] for q in native['queries'] if 'plans' in q
                for opt in scheduled.options(q,native['table_rows']) for p in opt['parameter_interface']['parameters']))
            self.assertTrue(any(q['parameter_interface']['guards'] for q in chosen))
            selected=out/'selected-certificates.egg'
            self.assertEqual(Path(manifest['certificate_library']),selected)
            programs=[json.loads(l[len('; @cell-program '):]) for l in selected.read_text().splitlines() if l.startswith('; @cell-program ')]
            for q in chosen:
                program=next(p for p in programs if p['ruleset']==q['ruleset'] and p['shape']['atoms']==q['plan']['shape']['atoms'])
                self.assertEqual(program['marker']['carried_slots'],q['plan']['carried_slots'])
                self.assertEqual(program['marker']['shared_marker'],q['plan'].get('shared_marker',False))
            # A new process needs only the selected library and new input, not
            # training facts, the selection JSON, or an online solver.
            run=subprocess.run([ROOT/'target/analyzer/debug/egg_layout','run-transfer',selected,
                ROOT/'experiments/observed_transfer_queries/tuple-updates.egg',tmp/'run'],cwd=ROOT,capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
            self.assertEqual(source.read_bytes(),(out/'program.egg').read_bytes())


    def test_incumbent_bound_keeps_joint_sharing_and_removes_only_losers(self):
        def private(c):return dict(costs=dict(storage=0,search=c,update=0),indexes=[],shared=None)
        shared=dict(costs=dict(storage=0,search=0,update=0),indexes=[],shared={'key':'m'},
            required_markers=[dict(definition={'key':'m'},costs=dict(storage=10,update=0))])
        groups=[({},[private(6),shared,private(1e30)],None) for _ in range(2)]
        kept,charges,bound=scheduled.bounded_layouts(groups,{},(1,1,1))
        self.assertEqual(bound['feasible_upper_bound'],12)
        self.assertEqual(bound['dominated_choices'],2)
        self.assertTrue(all(shared in opts for _,opts,_ in kept))
        self.assertEqual(charges['m']['storage'],10)
        solver.select('scip')
        with tempfile.TemporaryDirectory() as tmp:
            result=scheduled.select([self.fixture()],dict(A=100000,Ctx=1000000,Ty=1000000,C=1000000),Path(tmp),(1,1,1))
        self.assertGreater(result['exact_elimination']['dominated_choices'],0)
        self.assertEqual(len(result['selected']),1)
