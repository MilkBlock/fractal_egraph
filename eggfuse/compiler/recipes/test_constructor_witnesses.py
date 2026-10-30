import copy,unittest
from .constructor_witnesses import constructor_witnesses

class WitnessTests(unittest.TestCase):
    def test_aliases_and_nested_constructor_reads(self):
        tables=[dict(name='C',kind='constructor',merge=None)]
        rule=dict(query=[dict(call={'table':'C'},args=[{'slot':0},{'slot':1}]),dict(call={'table':'C'},args=[{'slot':1},{'slot':2}])],effects=[
            dict(kind='copy',dst=3,arg={'slot':0}),
            dict(kind='construct',dst=4,call={'table':'C'},args=[{'slot':3}]),
            dict(kind='construct',dst=5,call={'table':'C'},args=[{'slot':4}]),
            dict(kind='set',call={'table':'C'},args=[{'slot':3}],value={'slot':5})])
        proofs=constructor_witnesses(rule,tables)
        self.assertEqual(proofs[1]['value'],{'slot':1});self.assertEqual(proofs[2]['value'],{'slot':2})
        self.assertEqual(proofs[3]['atom'],0)
        other=copy.deepcopy(rule);other['effects'][3]['args']=[{'slot':99}]
        self.assertNotIn(3,constructor_witnesses(other,tables))
        for kind,merge in [('function',None),('constructor','new'),('constructor','old')]:
            self.assertFalse(constructor_witnesses(rule,[dict(name='C',kind=kind,merge=merge)]))

if __name__=='__main__':unittest.main()
