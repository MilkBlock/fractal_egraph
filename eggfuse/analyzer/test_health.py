import json
from pathlib import Path
import tempfile
import unittest
from .health import inspect, publish, validate
from eggfuse.compiler.passes import PassManager

class BakeHealthTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
    def tearDown(self):self.temp.cleanup()
    def write(self, jobs=(), cells=()):
        for name, value in {
            'analysis.json':{'status':'complete','saturated_rule_composition':{'jobs':list(jobs)}},
            'cell-recipes.json':{'cells':list(cells)},
            'cell-device-recipes.json':{'programs':[],'instances':[]},
        }.items():(self.root/name).write_text(json.dumps(value))
    def test_native_environment_failure_is_not_a_valid_cache(self):
        self.write([{'state':'Rejected','reason':'Undefined sort CellStructuralKey'}])
        with self.assertRaisesRegex(ValueError,'Rebuild the analyzer'):publish(self.root)
        self.assertFalse(json.loads((self.root/'bake-health.json').read_text())['environment_valid'])
        with self.assertRaisesRegex(ValueError,'environment failed'):validate(self.root)
    def test_replay_failure_and_observation_limit_are_reported(self):
        self.write([{'state':'Rejected','reason':'Command should have failed'},
                    {'state':'Unfinished','reason':'time limit'}])
        r=publish(self.root);self.assertEqual(r['job_states'],{'Rejected':1,'Unfinished':1})
        self.assertEqual(r['certified_endpoints'],0);self.assertFalse(r['live_substitution_authorized'])
        (self.root/'bake-health.json').unlink()
        p=self.root/'analysis.json';a=json.loads(p.read_text());a['status']='observed_unfinished';p.write_text(json.dumps(a))
        self.assertEqual(publish(self.root)['analysis_status'],'observed_unfinished')
    def test_observed_endpoint_is_not_a_certificate(self):
        self.write([{'state':'Saturated'}],[{'completion_status':'observed_endpoint_only',
            'completion':{'certified':False},'completion_reason':{'reason':'ordered merge'}}])
        r=publish(self.root);self.assertEqual(r['certification_rejections'],{'ordered merge':1})
        self.assertEqual(r['certified_endpoints'],0);self.assertEqual(validate(self.root),r)
    def test_saved_health_is_checked_against_actual_files(self):
        self.write();publish(self.root)
        p=self.root/'cell-device-recipes.json';p.write_text('{"programs":[{}],"instances":[]}')
        with self.assertRaisesRegex(ValueError,'differs'):validate(self.root)
    def test_failing_analysis_never_gets_successful_pass_receipt(self):
        self.write([{'state':'Rejected','reason':'Undefined sort X'}])
        pm=PassManager(self.root/'pipeline')
        with self.assertRaisesRegex(ValueError,'environment failed'):
            pm.run('analyze',{}, {},[],[],lambda _:None,lambda _:validate(self.root))
        self.assertEqual(json.loads((self.root/'pipeline/pipeline.json').read_text())['failed']['pass_name'],'analyze')
        self.assertEqual(pm.receipts,[])
    def test_zero_cell_input_is_allowed_without_closure_credit(self):
        self.write();r=validate(self.root)
        self.assertTrue(r['environment_valid']);self.assertEqual(r['jobs'],0)
        self.assertEqual(r['certified_endpoints'],0)

if __name__=='__main__':unittest.main()
