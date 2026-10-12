"""Count canonical storage and isolate diagnostics from the timed path."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parent))
from spine_ablation import ROOT,RUNNER,clone_inputs


class AblationEvidence(unittest.TestCase):
    def test_clones_keep_numeric_conditions_and_rename_global_bindings(self):
        var=lambda n:dict(kind='var',name=n)
        leaf=dict(kind='call',head='Leaf',args=[dict(kind='literal',sort='String',text='"x"')])
        number=dict(kind='literal',sort='i64',text='7')
        data=dict(actions=[dict(action=dict(kind='let',name='root',expr=leaf)),
            dict(action=dict(kind='expr',expr=dict(kind='call',head='Use',args=[var('root'),number])))])
        before=copy.deepcopy(data);result=clone_inputs(data,2)
        self.assertEqual(data,before)
        self.assertEqual([result[i]['name'] for i in (0,2)],['instance_0_root','instance_1_root'])
        for i in (1,3):
            self.assertEqual(result[i]['expr']['args'][0]['name'],result[i-1]['name'])
            self.assertEqual(result[i]['expr']['args'][1],number)
        self.assertNotEqual(result[0]['expr'],result[2]['expr'])

    def test_native_diagnostics_include_raw_and_canonical_rows(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='ablation-counts-') as temp:
            folder=Path(temp)
            (folder/'env.egg').write_text('''
(datatype Term (Leaf String) (A Term))
(datatype Spine-Term (Spine-Leaf String) (Spine-A Spine-Term) (Spine-Cano-A Spine-Term))
(sort Spine-Cell)
(constructor Spine-Summary-0 (Spine-Term) Spine-Cell)
(ruleset r)
(rule ((= a (Spine-A x))) ((Spine-Cano-A x) (Spine-Summary-0 a)) :ruleset r :name "create")
''')
            (folder/'input.egg').write_text('(Spine-A (Spine-Leaf "x"))')
            manifest=dict(environment=str(folder/'env.egg'),input=str(folder/'input.egg'),
                          run='(run-schedule (saturate r))',constructors=['Leaf','A'])
            path=folder/'manifest.json';path.write_text(json.dumps(manifest))
            def run(args):
                r=subprocess.run([RUNNER,*args],cwd=ROOT,check=True,text=True,capture_output=True)
                return json.loads(r.stdout.splitlines()[-1])
            checked=run(['bench-probes',path,folder/'graph.json'])
            v=checked['verification']
            self.assertEqual(v['spine']['enodes'],3)
            self.assertEqual(v['raw_spine']['enodes'],2)
            self.assertEqual(v['canonical_spine']['enodes'],1)
            self.assertEqual(v['native_total_rows_before_export'],4)
            self.assertEqual(v['active_summaries'],1)
            self.assertEqual(v['rule_matches'],{'create':1})
            p=v['run_probe_counters']
            # A one-atom scan may need no Join probe at all; zero is valid.
            self.assertIn('probes',p)
            self.assertEqual(p['candidate_rows'],p['  candidate_rows enumerated (for_each)']+p['  candidate_rows from lookups'])
            path.write_text(json.dumps(dict(manifest,counts=True)))
            counts=run(['bench-probes',path])
            self.assertEqual(counts['verification']['check_mode'],'counts_only')
            self.assertEqual(counts['verification']['spine'],v['spine'])
            self.assertEqual(counts['verification']['restored'],v['restored'])
            path.write_text(json.dumps(manifest))
            timed=run(['bench',path])
            for phase in ('run_engine_seconds','export_engine_seconds'):
                self.assertEqual(set(timed[phase]),{'search_apply','merge','rebuild'})
                self.assertTrue(all(v>=0 for v in timed[phase].values()))
            self.assertTrue(all(v==0 for v in timed['export_engine_seconds'].values()))
            self.assertEqual(timed['matches'],checked['matches'])
            self.assertIsNone(timed['verification'])
            self.assertAlmostEqual(timed['seconds'],sum(timed[k] for k in ('setup_seconds','input_seconds','run_seconds','export_seconds')))

    def test_native_short_names_count_every_storage_category(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='bang-counts-') as tmp:
            folder=Path(tmp)
            (folder/'env.egg').write_text("""
(datatype Term (Leaf String) (A Term))
(datatype Term! (Leaf! String) (A! Term!) (A!! Term!))
(sort Cell!)
(constructor Summary-0! (Term!) Cell!)
(constructor Summary-1! (Term!) Cell!)
(ruleset Run!)
(rule ((= a (A! x))) ((A!! x) (Summary-0! a)) :ruleset Run! :name "create")
(rule ((Summary-0! a)) ((Summary-1! a) (subsume (Summary-0! a))) :ruleset Run! :name "retire")
""")
            (folder/'input.egg').write_text('(A! (Leaf! "x"))')
            manifest=dict(environment=str(folder/'env.egg'),input=str(folder/'input.egg'),
                run='(run-schedule (saturate Run!))',constructors=['Leaf','A'],counts=True)
            path=folder/'counts.json';path.write_text(json.dumps(manifest))
            result=subprocess.run([RUNNER,'bench-probes',path],cwd=ROOT,check=True,text=True,capture_output=True)
            v=json.loads(result.stdout.splitlines()[-1])['verification']
            self.assertEqual(v['raw_spine']['enodes'],2)
            self.assertEqual(v['canonical_spine']['enodes'],1)
            self.assertEqual(v['active_summaries'],1)
            self.assertEqual(v['subsumed_summaries'],1)
            self.assertEqual(v['native_total_rows_before_export'],5)


if __name__=='__main__':unittest.main()
