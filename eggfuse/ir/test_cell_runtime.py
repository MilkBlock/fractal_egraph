import copy,unittest
from eggfuse.ir.cell_runtime import build,validate
from eggfuse.ir.recipe import digest

class ContractTests(unittest.TestCase):
 def core(self):
  e={'name':'E','kind':'eq'};v=lambda n:{'var':n,'sort':e}
  return {'schema':'egglog-device-core/v1','tables':[{'name':'N','kind':'constructor','schema':[e],'merge_expr':None}],
    'rulesets':[{'name':'s','rules':[{'name':'r','body':[{'call':{'table':'N'},'args':[v('x')]}],'head':[]}]}],
    'initial':[],'schedule':[{'run':'s'}]}
 def test_roundtrip(self):self.assertEqual(validate(build(self.core())),self.core())
 def test_changed_entry_rejected_even_with_new_checksum(self):
  p=build(self.core());p['templates'][0]['direct_entry']=False;p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
  with self.assertRaises(ValueError):validate(p)
 def test_changed_initial_rejected(self):
  p=build(self.core());p['initial']=[{'table':'N'}];p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
  with self.assertRaises(ValueError):validate(p)
 def chain_core(self):
  e={'name':'E','kind':'eq'};v=lambda n:{'var':n,'sort':e}
  atom=lambda t,*ns:{'call':{'table':t},'args':[v(n) for n in ns]}
  head=lambda t,x,y:{'op':'let','call':{'table':t},'args':[v(x)],'dst':y,'sort':e}
  return {'schema':'egglog-device-core/v1','tables':[{'name':t,'kind':'constructor','schema':[e]*arity,'merge_expr':None} for t,arity in [('A',1),('B',2),('C',2)]],
   'rulesets':[{'name':'first','rules':[{'name':'produce','body':[atom('A','x')],'head':[head('B','x','b')]}]},
    {'name':'second','rules':[{'name':'consume','body':[atom('B','x','b'),atom('A','x')],'head':[head('C','b','c')]}]}],
   'initial':[],'schedule':[{'sequence':[{'run':'first'},{'run':'second'}]}]}
 def test_cross_rule_recipe(self):
  p=build(self.chain_core());routes=p['multi_stage']['routes'];self.assertEqual(len(routes),1)
  self.assertEqual(routes[0]['steps'],[{'atom':1,'source_kind':'query','source_index':0}]);validate(p)
 def test_forged_continuation_rejected(self):
  p=build(self.chain_core());p['multi_stage']['routes'][0]['steps'][0]['source_index']=99
  p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
  with self.assertRaises(ValueError):validate(p)
 def test_changed_stage_permissions_rejected(self):
  p=build(self.chain_core());p['multi_stage']['control']=[];p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
  with self.assertRaises(ValueError):validate(p)
 def test_forged_point_program_rejected(self):
  p=build(self.chain_core());p['multi_stage']['entries'][0]['atoms']=[99];p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
  with self.assertRaises(ValueError):validate(p)
 def test_legacy_artifact_has_no_synthetic_guarantees(self):
  p=build(self.core());p.pop('multi_stage');p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'});self.assertEqual(validate(p),self.core())
 def test_union_entry_is_derived_from_typed_wiring(self):
  p=build(self.chain_core());self.assertTrue(p['multi_stage']['union_entries'])
  p['multi_stage']['union_entries'][0]['columns'][0]+=1;p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
  with self.assertRaises(ValueError):validate(p)
 def test_legacy_multistage_can_be_reemitted(self):
  p=build(self.chain_core());p['multi_stage'].pop('union_entries');p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
  self.assertEqual(validate(p),self.chain_core())
if __name__=='__main__':unittest.main()
