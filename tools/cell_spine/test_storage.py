"""Concrete storage must affect selection and count sharing/retirement correctly."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import sys
from .compiler import ROOT
from .pricing import RuntimePricing


class StorageSelection(unittest.TestCase):
    def test_native_storage_can_reverse_marker_selection(self):
        chosen={0:dict(marker_layouts=[dict(id='narrow',columns=1),dict(id='wide',columns=3)])}
        bundle=dict(chosen=chosen,consumers=[])
        calls=[]
        def measure(bundle,selected,folder):
            name=selected[0]['id'];calls.append(name)
            return dict(value=100 if name=='narrow' else 4)
        with tempfile.TemporaryDirectory() as d:
            oracle=RuntimePricing(lambda *_:bundle,{},Path(d),(1,1,1),storage=measure)
            plan=dict(selected=[0],implementations=[])
            _,result=oracle.optimize(plan)
            self.assertEqual(result['marker_layouts'],['wide'])
            self.assertEqual(result['value'],7)
            self.assertEqual(result['costs']['instance_value_slots'],4)
            oracle.optimize(plan)
            self.assertEqual(len(calls),2)  # exact plan cache, no repeated native execution

    def test_storage_errors_are_not_silently_replaced_by_structural_cost(self):
        def fail(*_):raise RuntimeError('native failure')
        bundle=dict(chosen={0:dict(marker_layouts=[dict(id='m',columns=1)])},consumers=[])
        with tempfile.TemporaryDirectory() as d:
            oracle=RuntimePricing(lambda *_:bundle,{},Path(d),(1,1,1),storage=fail)
            with self.assertRaisesRegex(RuntimeError,'native failure'):
                oracle.optimize(dict(selected=[0],implementations=[]))

    def test_proven_lower_bound_skips_native_execution_but_does_not_cache_a_solution(self):
        calls=[]
        def measure(*_):calls.append(True);return dict(value=4)
        bundle=dict(chosen={0:dict(marker_layouts=[dict(id='m',columns=10)])},consumers=[])
        with tempfile.TemporaryDirectory() as d:
            oracle=RuntimePricing(lambda *_:bundle,{},Path(d),(1,1,1),storage=measure)
            plan=dict(selected=[0],implementations=[])
            _,bound=oracle.price_below(plan,9)
            self.assertTrue(bound['lower_bound_only']);self.assertFalse(calls)
            _,exact=oracle.optimize(plan)
            self.assertEqual(exact['value'],14);self.assertEqual(len(calls),1)


class NativeStorage(unittest.TestCase):
    def measure(self,initial,rule=''):
        environment='''(datatype T (Leaf String) (Pair T T))
(sort Cell!) (constructor Summary-0! (T) Cell!)
(ruleset Run!)
'''+rule
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'input.json'
            path.write_text(json.dumps(dict(environment=environment,input=initial,run='(run-schedule (saturate Run!))')))
            result=subprocess.run([ROOT/'target/release/examples/native_math_baseline','storage',path],
                                  check=True,capture_output=True,text=True)
        return json.loads(result.stdout.splitlines()[-1])

    def test_duplicate_input_is_shared_but_different_binding_costs_more(self):
        seed='(Summary-0! (Pair (Leaf "x") (Leaf "y")))'
        one=self.measure(seed);twice=self.measure(seed+'\n'+seed)
        self.assertEqual(one['retained'],twice['retained'])
        different=self.measure(seed+'\n(Summary-0! (Pair (Leaf "x") (Leaf "z")))')
        self.assertEqual(one['retained']['rows'],4)
        self.assertEqual(different['retained']['rows'],7) # shared Leaf x, not two separate trees
        self.assertGreater(different['checkpoint_peak_value_slots'],one['checkpoint_peak_value_slots'])

    def test_retirement_removes_matching_not_storage_and_union_uses_native_keys(self):
        rule='(rule ((= m (Summary-0! x))) ((subsume (Summary-0! x))) :ruleset Run!)'
        result=self.measure('(Summary-0! (Leaf "x"))',rule)
        self.assertEqual(result['initial']['active_markers'],1)
        self.assertEqual(result['retained']['active_markers'],0)
        self.assertEqual(result['retained']['retired_markers'],1)
        self.assertGreater(result['retained']['retired_marker_value_slots'],0)
        self.assertEqual(result['initial']['rows'],result['retained']['rows'])
        seed='(let x (Leaf "x")) (let y (Leaf "y")) (Pair x x) (Pair y y)'
        separate=self.measure(seed);unioned=self.measure(seed+' (union x y)')
        self.assertEqual(separate['retained']['tables']['Pair']['rows'],2)
        self.assertEqual(unioned['retained']['tables']['Pair']['rows'],1)
        self.assertIn('materialized_indexes',result['retained'])
        self.assertIn('join/rebuild index bytes',result['unpriced'])

    def test_instance_compiler_prices_and_emits_the_same_input_and_retained_rows(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='instance-storage-test-') as d:
            out=Path(d)/'bake'
            result=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',
                ROOT/'experiments/cell_spine/fixtures/input_spine_left.egg','--output',out,'--skip-build','--storage-cost','instances'],
                capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            report=json.loads((out/'report.json').read_text())
            self.assertEqual(report['storage_cost'],'instances')
            priced=report['instance_storage']['selected'];native=priced['native']
            actual={k:v for k,v in report['source_storage']['native_tables_before_export'].items() if v}
            self.assertEqual(actual,{k:v['rows'] for k,v in native['retained']['tables'].items()})
            self.assertEqual(report['plan']['costs']['instance_value_slots'],native['checkpoint_peak_value_slots'])
            body=(out/'runtime.egg').read_text().removeprefix((out/'library.egg').read_text())
            if priced['initial'] is not None:
                for line in priced['initial']:self.assertIn(line,body)
            costs=report['plan']['costs'];w=report['plan']['weights']
            expected=w['storage']*(costs['template_nodes']+costs['marker_columns']+costs['instance_value_slots'])
            expected+=w['search']*costs['lookup_atoms_and_guards']+w['update']*costs['write_atoms_and_unions']
            self.assertEqual(expected,report['plan']['objective'])
            checks=json.loads((out/'validation.json').read_text())['cases']
            self.assertTrue(all(c['comparison']['status']=='equivalent' for c in checks))


if __name__=='__main__':unittest.main()
