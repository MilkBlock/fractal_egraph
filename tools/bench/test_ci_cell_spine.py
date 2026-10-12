"""CI contract tests do not require a GitHub account or a solver run."""
import copy
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from tools.bench.ci_cell_spine import replicas,check_renaming,measure,read_result,PHASES,ENGINE_PHASES
from tools.bench.check_ci_cell_spine import compare


def fixture():
    timing={k:1. for k in PHASES+ENGINE_PHASES};timing.update(process_seconds=4.,peak_rss_bytes=32*1024**2)
    return dict(schema='cell-ci-bench/v1',suite_sha256='suite',copies=128,threads=1,
        environment=dict(os='Linux',arch='x86_64',cpu='cpu',python='3.12',rust='1.94',gurobi='13',runner_image='ubuntu24'),
        cases={'test':dict(source_sha256='source',copies=128,bytes={'library.egg':100},
            bake=dict(capture_mode='fresh',compiler_seconds=1.,process_seconds=1.2,phase_seconds={'capture':1.}),
            modes={'native':dict(timing=timing,size=dict(physical_rows=100,restored_enodes=80,restored_eclasses=50))})})


class RegressionGates(unittest.TestCase):
    def test_same_measurements_pass(self):self.assertFalse(compare(fixture(),fixture())['failures'])
    def test_size_and_individual_phases_are_gated(self):
        for edit,label in [(lambda c:c['bytes'].update({'library.egg':120}),'bytes'),
                           (lambda c:c['modes']['native']['size'].update(physical_rows=120),'physical_rows'),
                           (lambda c:c['modes']['native']['timing'].update(run_seconds=2.),'run_seconds'),
                           (lambda c:c['bake']['phase_seconds'].update(capture=2.),'capture')]:
            now=fixture();edit(now['cases']['test'])
            self.assertTrue(any(label in f for f in compare(fixture(),now)['failures']))
    def test_logical_shrinkage_is_not_reported_as_compression(self):
        now=fixture();now['cases']['test']['modes']['native']['size']['restored_enodes']=79
        self.assertTrue(any('logical size changed' in f for f in compare(fixture(),now)['failures']))
    def test_changed_cpu_skips_time_but_not_size_gates(self):
        now=fixture();now['environment']['cpu']='other'
        now['cases']['test']['modes']['native']['timing']['run_seconds']=20.
        result=compare(fixture(),now);self.assertFalse(result['timings_comparable']);self.assertFalse(result['failures'])
        now['cases']['test']['bytes']['library.egg']=200
        self.assertTrue(compare(fixture(),now)['failures'])
    def test_tiny_phase_noise_has_an_absolute_floor(self):
        before=fixture();after=fixture()
        before['cases']['test']['modes']['native']['timing']['run_seconds']=.00001
        after['cases']['test']['modes']['native']['timing']['run_seconds']=.0001
        self.assertFalse(compare(before,after)['failures'])
    def test_missing_case_mode_phase_or_invalid_number_fails(self):
        edits=[lambda r:r['cases'].clear(),lambda r:r['cases']['test']['modes'].clear(),
               lambda r:r['cases']['test']['bake']['phase_seconds'].clear()]
        for bad in (-1,math.inf,math.nan,None,True):
            edits.append(lambda r,bad=bad:r['cases']['test']['modes']['native']['timing'].update(run_seconds=bad))
        for edit in edits:
            now=fixture();edit(now)
            with self.assertRaises((ValueError,KeyError)):compare(fixture(),now)
    def test_first_run_does_not_claim_a_historical_comparison(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'current.json').write_text(json.dumps(fixture()))
            cmd=[sys.executable,'tools/bench/check_ci_cell_spine.py','--baseline',p/'missing.json',
                 '--current',p/'current.json','--output',p/'comparison.json']
            subprocess.run(cmd,check=True,capture_output=True)
            self.assertEqual(json.loads((p/'comparison.json').read_text())['status'],'no_baseline')
            self.assertFalse((p/'missing.json').exists())


class MeasurementAndWorkload(unittest.TestCase):
    def test_single_child_measurement_and_failure_are_not_silent(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'run'
            r=measure([sys.executable,'-c','print(\'{"ok": true}\')'],p)
            self.assertGreater(r['peak_rss_bytes'],0);self.assertTrue(read_result(p)['ok'])
            with self.assertRaises(RuntimeError):measure([sys.executable,'-c','raise SystemExit(2)'],p)
    def test_replication_preserves_aliases_and_numeric_constants(self):
        acts=[dict(kind='let',name='x',expr=dict(kind='call',head='Leaf!',args=[dict(kind='literal',sort='String',text='"a"')])),
              dict(kind='expr',expr=dict(kind='call',head='Pair!',args=[dict(kind='var',name='x'),dict(kind='var',name='x'),dict(kind='literal',sort='i64',text='7')]))]
        before=copy.deepcopy(acts);result=replicas(acts,2)
        self.assertEqual(acts,before)
        self.assertNotEqual(result[0]['name'],result[2]['name'])
        self.assertEqual(result[1]['expr']['args'][:2],[dict(kind='var',name=result[0]['name'])]*2)
        self.assertEqual(result[1]['expr']['args'][2]['text'],'7')
    def test_fixed_strings_or_primitives_prevent_invalid_scaling(self):
        literal=dict(kind='literal',sort='String',text='"fixed"')
        rule=dict(body=[dict(kind='fact',expr=dict(kind='call',head='Leaf',args=[literal]))],head=[])
        with self.assertRaises(ValueError):check_renaming(dict(rules=[rule],signatures={'Leaf':{}}),[])
        rule['body'][0]['expr']=dict(kind='call',head='str-concat',args=[])
        with self.assertRaises(ValueError):check_renaming(dict(rules=[rule],signatures={}),[])


if __name__=='__main__':unittest.main()
