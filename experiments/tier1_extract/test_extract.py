import unittest
from extract import select,extract

class ExtractTests(unittest.TestCase):
    def test_selects_finite_existing_representative(self):
        nodes={'loop':{'eclass':'Comb-a','op':'Loop','children':['loop']},'empty':{'eclass':'Comb-a','op':'Empty','children':[]}}
        chosen,cost=select(nodes)
        self.assertEqual(chosen['Comb-a'],'empty');self.assertEqual(cost['Comb-a'],1)
    def test_shared_parent_defined_once(self):
        nodes={'a':{'eclass':'Comb-a','op':'Empty','children':[]},'b':{'eclass':'Comb-b','op':'Wrapper','children':['a','a']}}
        code,r=extract({'native_egraph':{'nodes':nodes},'templates':[{'id':'Comb-a'},{'id':'Comb-b'}],'instances':[]})
        self.assertEqual(len(r['definitions']),2)
        self.assertIn('(Wrapper $comb_0000 $comb_0000)',code)
    def test_unproductive_cycle_is_rejected(self):
        nodes={'a':{'eclass':'Comb-a','op':'Loop','children':['a']}}
        with self.assertRaises(AssertionError):extract({'native_egraph':{'nodes':nodes},'templates':[{'id':'Comb-a'}],'instances':[]})
if __name__=='__main__':unittest.main()
