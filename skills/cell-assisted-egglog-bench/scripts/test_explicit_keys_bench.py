import copy
import unittest
import explicit_keys_bench as bench

class Gate(unittest.TestCase):
    def fixture(self):
        lane=dict(cell_assistance=True,cell_query_rules=5,cell_query_bindings=9,
                  skipped_saturations=2,joint={'stats':{'boundaries':2}},shared_markers=[],ratio_to_legacy=1.0)
        return dict(lanes={n:copy.deepcopy(lane) for n in ['legacy','explicit-sk','explicit-markers']},graph_passed=True,extraction_passed=True)
    def test_gate_needs_same_services_and_both_graph_and_extraction(self):
        self.assertTrue(bench.evaluate(self.fixture())['ready_to_merge'])
        for k in ['graph_passed','extraction_passed']:
            x=self.fixture();x[k]=False;self.assertFalse(bench.evaluate(x)['ready_to_merge'])
        x=self.fixture();x['lanes']['explicit-sk']['cell_query_bindings']=8
        self.assertFalse(bench.evaluate(x)['ready_to_merge'])
    def test_no_tolerance_hides_a_slowed_lane(self):
        x=self.fixture();x['lanes']['explicit-markers']['ratio_to_legacy']=1.01
        self.assertFalse(bench.evaluate(x)['ready_to_merge'])
