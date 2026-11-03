import copy,unittest
from experiments.cell_runtime.compositions_bench import merge_gate

def result(source='primary',time=.8,work=.3,rss=1.0):
 return {'provenance':{'source_sha256':source},'verification':{'all_stage_graphs_and_bindings_equal':True},
         'ratios':{'inference':time,'rss':rss,'work_reductions':{'key_lookups':work}},
         'lanes':{s:{'measurement':{'binary_sha256':'same'}} for s in ('off','on')}}
class GateTests(unittest.TestCase):
 def test_slowdown_is_not_paid_for_by_hits_or_work_reduction(self):
  self.assertFalse(merge_gate(result(time=1.1,work=.9),[result('g1'),result('g2')])['merge_allowed'])
 def test_all_gates_required(self):
  r=result();guards=[result('g1',time=1.04),result('g2')]
  self.assertTrue(merge_gate(r,guards)['merge_allowed'])
  self.assertFalse(merge_gate(r,guards[:1])['merge_allowed'])
  self.assertFalse(merge_gate(r,[guards[0],guards[0]])['merge_allowed'])
  guards[0]['verification']=None;self.assertFalse(merge_gate(r,guards)['merge_allowed'])
 def test_memory_and_guard_regression_block_admission(self):
  self.assertFalse(merge_gate(result(rss=1.2),[result('g1'),result('g2')])['merge_allowed'])
  self.assertFalse(merge_gate(result(),[result('g1',time=1.1),result('g2')])['merge_allowed'])
if __name__=='__main__':unittest.main()
