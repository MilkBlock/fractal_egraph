import copy,unittest
from eggfuse.ir.cell_runtime import build,validate
from eggfuse.ir import test_cell_runtime as fixtures
from eggfuse.ir.recipe import digest

class EntryScheduleTests(unittest.TestCase):
    def test_shared_nodes_keep_original_consumer_projections(self):
        p=build(fixtures.ContractTests().chain_core());plan=p['entry_schedule']
        self.assertTrue(any(len(n['consumers'])>1 for n in plan['nodes']))
        self.assertEqual(plan['control'],p['recipe']['control']);validate(p)
    def test_names_do_not_select_the_optimization(self):
        core=fixtures.ContractTests().chain_core();renamed=copy.deepcopy(core);names={'A':'SourcePort','B':'MiddlePort','C':'OutputPort'}
        def walk(x):
            if isinstance(x,dict):
                if 'table' in x:x['table']=names.get(x['table'],x['table'])
                for v in x.values():walk(v)
            elif isinstance(x,list):
                for v in x:walk(v)
        walk(renamed)
        for t in renamed['tables']:t['name']=names[t['name']]
        a,b=build(core)['entry_schedule'],build(renamed)['entry_schedule']
        self.assertEqual([n['consumers'] for n in a['nodes']],[n['consumers'] for n in b['nodes']])
    def test_forged_projection_is_rejected_after_resigning(self):
        p=build(fixtures.ContractTests().chain_core());p['entry_schedule']['nodes'][0]['consumers'][0]['slots'][0]['port']=999
        p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
        with self.assertRaises(ValueError):validate(p)
