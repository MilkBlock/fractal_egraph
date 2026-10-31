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
if __name__=='__main__':unittest.main()
