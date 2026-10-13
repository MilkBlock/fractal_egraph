"""Active canonical markers, native-state equivalence and union correlations."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from .compiler import ROOT, load_candidates
from .markers import RUNTIME_SCHEDULE, layouts
from .pricing import audit,cost
from .terms import text,walk
from .test_support import marker_pattern


class ActiveMarkers(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='marker-test-')
        cls.folder=Path(cls.tmp.name);cls.outputs={}
        for name in ('assoc','root_alias','shared_tail','long_spine_graft'):
            out=cls.folder/name
            r=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',
                ROOT/f'experiments/cell_spine/fixtures/{name}.egg','--output',out,'--skip-build','--storage-cost','structural',
                *(['--search-weight','0'] if name=='root_alias' else [])],
                cwd=ROOT,capture_output=True,text=True)
            if r.returncode:raise AssertionError(r.stdout+r.stderr)
            cls.outputs[name]=out

    @classmethod
    def tearDownClass(cls):cls.tmp.cleanup()

    def verify(self,fixture,name,reference,compressed):
        out=self.outputs[fixture];base=json.loads((out/'work/validation.json').read_text())
        manifest=dict(constructors=base['constructors'],library=base['library'],cases=[dict(name=name,
            reference=reference,compressed=compressed,restore='(run-schedule (saturate Export!))')])
        path=self.folder/(name+'.json');path.write_text(json.dumps(manifest))
        dest=self.folder/(name+'-result.json')
        r=subprocess.run([ROOT/'target/release/examples/spine_validate',path,dest],capture_output=True,text=True)
        self.assertEqual(r.returncode,0,r.stderr)
        return json.loads(dest.read_text())['cases'][0]

    def test_no_read_tables_and_one_active_canonical_cell(self):
        for out in self.outputs.values():
            lib=(out/'library.egg').read_text()
            self.assertNotIn('Read-!',lib)
            self.assertNotIn('Wanted-!',lib)
            for c in json.loads((out/'validation.json').read_text())['cases']:
                self.assertEqual(c['comparison']['status'],'equivalent')
                self.assertFalse(c['original_rules_executed'])
                self.assertEqual(c['physical_original_rows_before_export'],0)
        c=json.loads((self.outputs['assoc']/'validation.json').read_text())['cases'][-1]
        self.assertEqual(c['original_constructor_rows'],15)
        self.assertEqual(c['active_summary_tables'],{'Summary-0!':0,'Summary-1!':1})
        self.assertEqual(sum(c['native_tables_before_export'].values()),c['native_total_rows_before_export'])
        # Both raw and Cano constructors belong in the reported physical count.
        self.assertTrue(any(k.endswith('!!') for k in c['spine_tables']))

    def test_solver_prices_final_consumers_and_selects_reusable_subtree_anchor(self):
        for out in self.outputs.values():
            report=json.loads((out/'report.json').read_text())
            emitted=json.loads((out/'work/marker-runtime.json').read_text())['rules']
            prices=report['plan']['pricing']['rule_costs']
            audit(emitted,prices)
            self.assertEqual(report['retirement']['cost_audit']['status'],'exact')
            weights=report['plan']['weights'];costs=report['plan']['costs']
            score=(weights['storage']*(costs['template_nodes']+costs['marker_columns']+costs.get('interface_columns',0))+
                   weights['search']*sum(cost(r)['search_atoms'] for r in emitted)+
                   weights['update']*sum(cost(r)['update_actions'] for r in emitted))
            self.assertAlmostEqual(score,report['plan']['objective'])
        r=json.loads((self.outputs['long_spine_graft']/'report.json').read_text())
        ls=r['retirement']['marker_layouts']
        self.assertEqual(ls['1']['anchor'],ls['0']['roots'][0])
        self.assertNotEqual(ls['1']['position'],0)
        prices=r['plan']['pricing']['rule_costs']
        self.assertTrue(any(name.startswith('retire_') for name in prices))
        self.assertTrue(any('_cover_' in name for name in prices))
        self.assertTrue(any(text(ls['1']['anchor']) in p['reused_subtrees'] for p in prices.values()))
        self.assertLess(r['plan']['objective'],57)
        self.assertTrue(all(l.get('representation')=='borrowed' for l in ls.values()))
        self.assertFalse(any(p['canonical_writes'] for p in prices.values()))
        self.assertEqual(prices['retire_0']['search_atoms'],2)

    def test_inherited_graft_has_one_extend_query_and_no_union(self):
        out=self.outputs['long_spine_graft']
        rules=json.loads((out/'work/marker-runtime.json').read_text())['rules']
        inherited=next(r for r in rules if r['name']=='impl_1_0_cover_1')
        calls=[e for f in inherited['body'] for root in (f['args'] if f['kind']=='eq' else [f['expr']])
               for e in walk(root) if e.get('head')=='Extend!']
        self.assertEqual(len(calls),1)
        self.assertFalse(any(a['kind']=='union' for a in inherited['head']))
        report=json.loads((out/'report.json').read_text())
        self.assertEqual(report['plan']['pricing']['rule_costs'][inherited['name']]['unions'],0)

    def test_all_admitted_storage_contracts_compose_and_export(self):
        # Exercise feasible but non-optimal mixed ABIs as well as the optimum.
        # A borrowed parent must still materialize a canonical child when that
        # is what the selected child reader contract requires.
        from . import compiler as c
        out=self.outputs['long_spine_graft'];work=self.folder/'storage-matrix';work.mkdir()
        report=json.loads((out/'report.json').read_text())
        cs,_=c.load_candidates(Path(report['analysis']),work)
        for candidate in cs:candidate['marker_layouts']=layouts(candidate)
        edges=c.transition_options(cs);c.price_options(edges,work)
        plan=report['plan'];bundle=c.prepare_runtime(cs,plan,edges,work)
        data=json.loads((out/'work/input.json').read_text());sig=dict(data['signatures'])
        env=c.commands(data,{'declaration','rule'})
        for candidate in cs:
            for op,shape in candidate['signatures'].items():
                if op not in sig:
                    env.append(f'(constructor {op} ({" ".join(shape["input"])}) {shape["output"]})');sig[op]=shape
        modes={f:{} for f in bundle['chosen']}
        for f,candidate in bundle['chosen'].items():
            for layout in candidate['marker_layouts']:
                modes[f].setdefault(layout.get('representation','canonical'),layout)
        self.assertEqual(set(modes[1]),{'canonical','borrowed','boundary','boundary_borrowed'})
        for a in modes[0].values():
            for b in modes[1].values():
                selected=dict(plan,marker_layouts=[a['id'],b['id']])
                library,*_=c.make_library(cs,selected,edges,env,sig,work,bundle)
                manifest=json.loads((out/'work/validation.json').read_text());manifest['library']=library
                path=work/'verify.json';path.write_text(json.dumps(manifest))
                run=subprocess.run([ROOT/'target/release/examples/spine_validate',path,work/'result.json'],capture_output=True,text=True)
                self.assertEqual(run.returncode,0,(a['id'],b['id'],run.stderr))

    def test_root_union_does_not_invent_a_sibling_input(self):
        layout=json.loads((self.outputs['root_alias']/'report.json').read_text())['retirement']['marker_layouts']['0']
        self.assertLess(layout['columns'],len(layout['fields']))  # Exercise erased inputs, not a fully stored tuple.
        source=(ROOT/'experiments/cell_spine/fixtures/root_alias.egg').read_text()
        prefix=source[:source.index('(A (Leaf "a"))')]
        code='''(let a (Leaf "a")) (let b (Leaf "b"))
(A a) (B a)
(run-schedule (saturate demo))
(A b) (union (A a) (A b)) (Gate b)
(run-schedule (saturate demo))
(fail (check (B b))) (fail (check (Hit b)))
'''
        compact=code.replace('(Leaf','(Leaf!').replace('(A ','(A! ').replace('(B ','(B! ').replace('(Gate ','(Gate! ')
        compact=compact[:compact.index('(fail (check')].replace('(run-schedule (saturate demo))',RUNTIME_SCHEDULE)
        self.verify('root_alias','root-union',prefix+code,compact)

    def test_shared_tail_keeps_the_other_instance_active(self):
        source=(ROOT/'experiments/cell_spine/fixtures/shared_tail.egg').read_text()
        prefix=source[:source.index('(let t')]
        code='''(let t (Tail (Leaf "shared")))
(let a (Leaf "a")) (let b (Leaf "b"))
(A t a) (A t b)
(run-schedule (saturate demo))
(Gate a)
(run-schedule (saturate demo))
(fail (check (Joined t b)))
'''
        compact=code[:code.index('(fail (check')]
        for op in ('Tail','Leaf','A','Gate'):compact=compact.replace('('+op+' ','('+op+'! ')
        compact=compact.replace('(run-schedule (saturate demo))',RUNTIME_SCHEDULE)
        c=self.verify('shared_tail','shared-tail',prefix+code,compact)
        self.assertEqual(c['active_summary_tables'],{'Summary-0!':1,'Summary-1!':1})

    def test_unmarked_canonical_node_is_not_an_ingress(self):
        out=self.outputs['shared_tail']
        source=(out/'library.egg').read_text()+'''
(A!! (Tail!! (Leaf! "t")) (Leaf! "z"))
'''+RUNTIME_SCHEDULE+'\n'
        for family in (0,1):source+=f'(fail (check {marker_pattern(out,family)}))\n'
        path=self.folder/'unmarked.egg';path.write_text(source)
        r=subprocess.run([ROOT/'target/release/examples/native_math_baseline',path],capture_output=True,text=True)
        self.assertEqual(r.returncode,0,r.stderr)

    def test_graft_reuses_whole_parent_without_reading_its_chain(self):
        out=self.outputs['long_spine_graft'];report=json.loads((out/'report.json').read_text())
        transition=next(t for t in report['transitions'] if t['parents'] and not t['inherited'])
        rule=report['retirement']['runtime_rules'][transition['id']][0]
        self.assertNotIn('Plus!!',rule)
        self.assertIn('(Extend! parent_0_root_0',rule)
        self.assertIn('(subsume (Summary-',rule)
        c=json.loads((out/'validation.json').read_text())['cases'][-1]
        self.assertEqual(c['original_constructor_rows'],18)
        self.assertEqual(c['restored_constructor_rows'],18)
        self.assertEqual(c['active_summary_tables'],{'Summary-0!':0,'Summary-1!':1})
        candidates,_=load_candidates(out/'analysis',out/'work')
        anchor_terms={text(l['anchor']) for c in candidates for l in layouts(c)}
        self.assertTrue(any(s.startswith('p') for s in anchor_terms))  # Input anchors are admitted; equivalent layouts may share one ABI.
        self.assertTrue(any(s.startswith('(Plus! p5 p6)') for s in anchor_terms))


if __name__=='__main__':unittest.main()
