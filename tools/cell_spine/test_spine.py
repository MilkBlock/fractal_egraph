import itertools
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from .compiler import ROOT, compatible_program
from .planner import solve


class Planning(unittest.TestCase):
    def test_shared_path_beats_independent_minimum(self):
        paths=[]
        for f in range(3):
            paths.extend([dict(id=f*2,family=f,nodes=[f'private-{f}'],lhs_atoms=1),
                          dict(id=f*2+1,family=f,nodes=['shared-a','shared-b'],lhs_atoms=2)])
        with tempfile.TemporaryDirectory() as d:
            r=solve(paths,[],Path(d),weights=(1,0,0))
        self.assertEqual(r['selected'],[1,3,5])
        self.assertEqual(r['objective'],2)

    def test_joint_cost_matches_exhaustive_optimum(self):
        paths=[dict(id=0,family=0,nodes=['a'],lhs_atoms=2),dict(id=1,family=0,nodes=['b','c'],lhs_atoms=1),
               dict(id=2,family=1,nodes=['a','d'],lhs_atoms=3),dict(id=3,family=1,nodes=['b','c'],lhs_atoms=2)]
        def objective(pair):
            p,q=pair
            return len(set(p['nodes'])|set(q['nodes'])) + p['lhs_atoms']+q['lhs_atoms']+len(set(q['nodes'])-set(p['nodes']))
        expected=min(map(objective,itertools.product(paths[:2],paths[2:])))
        with tempfile.TemporaryDirectory() as d:
            result=solve(paths,[(0,1)],Path(d))
        self.assertEqual(result['objective'],expected)
        self.assertEqual(result['bound'],expected)


class NativePipeline(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='spine-test-')
        cls.folder=Path(cls.tmp.name)
        cls.outputs={}
        for name in ('assoc','union','multiple_types','default_ruleset','merged_ports','relation_fallback','function_fallback'):
            output=cls.folder/name
            subprocess.run([__import__('sys').executable,'-m','tools.cell_spine.compiler',
                '--source',f'experiments/cell_spine/fixtures/{name}.egg','--output',str(output),'--skip-build'],cwd=ROOT,check=True,
                stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            cls.outputs[name]=output

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_real_analysis_and_optimal_selection(self):
        d=json.loads((self.outputs['assoc']/'report.json').read_text())
        self.assertEqual(d['status'],'compiled')
        self.assertEqual(d['plan']['status'],'optimal')
        self.assertTrue(d['transitions'])
        self.assertGreater(len(d['paths']),len(d['plan']['selected']))

    def test_isolation_and_full_state_equivalence(self):
        for name in ('assoc','union','multiple_types','default_ruleset','merged_ports'):
            report=json.loads((self.outputs[name]/'validation.json').read_text())
            for case in report['cases']:
                self.assertEqual(case['physical_original_rows_before_export'],0)
                self.assertEqual(case['comparison']['status'],'equivalent')

    def test_functions_and_relations_use_verified_stage_compression(self):
        for name in ('relation_fallback','function_fallback'):
            out=self.outputs[name]
            d=json.loads((out/'report.json').read_text())
            self.assertEqual(d['status'],'compiled_stages')
            self.assertFalse(d['generalizes_bindings'])
            self.assertTrue((out/'selection.lp').is_file())
            v=json.loads((out/'validation.json').read_text())
            self.assertTrue(v['cases'])
            self.assertTrue(all(c['comparison']['status']=='equivalent' for c in v['cases']))

    def test_invalid_union_is_rejected(self):
        output=self.outputs['assoc']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=next(c for c in manifest['cases'] if c['name']=='path-0')
        case['compressed']+='\n(union (Spine-RipenInput 0) (Spine-RipenInput 1))\n'
        manifest['cases']=[case]
        path=self.folder/'bad.json'; path.write_text(json.dumps(manifest))
        result=subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'bad-report.json'],
                              capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('full closure mismatch',result.stderr)

    def test_ordinary_materialization_is_rejected(self):
        output=self.outputs['assoc']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=manifest['cases'][0]
        case['compressed']+='\n(Leaf "unexpected")\n'
        manifest['cases']=[case]
        path=self.folder/'expanded.json';path.write_text(json.dumps(manifest))
        result=subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'expanded-report.json'],
                              capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('original constructors materialized',result.stderr)

    def test_late_boundary_input_uses_existing_parent(self):
        output=self.outputs['assoc']
        lib=(output/'library.egg').read_text()
        # The generated rule recognizes a parent Summary, then its new coarse
        # context. The old context is installed and run before the new fact.
        case=dict(name='late-boundary',reference='''
(datatype Term (Leaf String) (Plus Term Term))
(ruleset algebra)
(rewrite (Plus a b) (Plus b a) :ruleset algebra)
(rewrite (Plus (Plus a b) c) (Plus a (Plus b c)) :ruleset algebra)
(Plus (Leaf "a") (Leaf "b"))
(run-schedule (saturate algebra))
(Plus (Plus (Leaf "b") (Leaf "a")) (Leaf "c"))
(run-schedule (saturate algebra))
''',compressed=lib+'''
(Spine-Plus (Spine-Leaf "a") (Spine-Leaf "b"))
(run-schedule (saturate Spine-Run))
(check (Spine-Summary-0 (Spine-Plus (Spine-Leaf "a") (Spine-Leaf "b")) (Spine-Leaf "a") (Spine-Leaf "b")))
(Spine-Plus (Spine-Plus (Spine-Leaf "b") (Spine-Leaf "a")) (Spine-Leaf "c"))
(run-schedule (saturate Spine-Run))
''',restore='(run-schedule (saturate Spine-Export))',expected_summary='Spine-Summary-1')
        manifest=dict(constructors=['Leaf','Plus','RipenInput'],cases=[case])
        path=self.folder/'late.json';path.write_text(json.dumps(manifest))
        subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'late-report.json'],check=True,stdout=subprocess.DEVNULL)

    def test_noninjective_boundary_bindings(self):
        output=self.outputs['assoc']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=next(c for c in manifest['cases'] if c['name']=='source')
        case['reference']=Path(case.pop('reference_path')).read_text().replace('"b"','"a"')
        case['compressed']=case['compressed'].replace('"b"','"a"')
        case['restore']=case['restore'].replace('"b"','"a"')
        manifest['cases']=[case]
        path=self.folder/'aliases.json';path.write_text(json.dumps(manifest))
        subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'aliases-report.json'],check=True,stdout=subprocess.DEVNULL)

    def test_original_rule_execution_is_rejected(self):
        output=self.outputs['assoc']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=manifest['cases'][0]
        case['restore']+='\n(run algebra 1)\n'
        manifest['cases']=[case]
        path=self.folder/'rules.json';path.write_text(json.dumps(manifest))
        result=subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'rules-report.json'],capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('may not execute the original ruleset',result.stderr)

    def test_merged_ports_remain_distinct_interfaces_and_union_before_export(self):
        output=self.outputs['merged_ports']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=next(c for c in manifest['cases'] if c['name']=='source')
        case['compressed']+='\n(check (= a b)) (check (= root a))\n'
        manifest['cases']=[case]
        path=self.folder/'visible-unions.json';path.write_text(json.dumps(manifest))
        subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'visible-unions-report.json'],check=True,stdout=subprocess.DEVNULL)


if __name__=='__main__':
    unittest.main()
