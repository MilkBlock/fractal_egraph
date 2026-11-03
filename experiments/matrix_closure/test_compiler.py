import itertools
import copy
import unittest
import numpy as np
from compiler import separator, compile_context

class Separators(unittest.TestCase):
    def test_every_boolean_corner(self):
        for k in range(9):
            s=separator(k);w=np.array(s['coefficients'],dtype=np.uint16).view(np.float16).astype(np.float32)
            for bits in itertools.product([0,1],repeat=k):
                actual=float(np.array([1,*bits],dtype=np.float32)@w)>s['threshold']
                self.assertEqual(actual,all(bits))
            self.assertGreater(s['margin'],s['roundoff_bound'])

    def test_large_arity_uses_sufficient_statistics(self):
        s=separator(40)
        self.assertEqual(s['truth_assignments'],1<<40)
        self.assertGreater(s['margin'],s['roundoff_bound'])

class Admission(unittest.TestCase):
    def setUp(self):
        eq={'name':'N','kind':'eq'};proof={'name':'Proof','kind':'eq'}
        self.core={'tables':[{'name':'Node','kind':'constructor','schema':[eq]},
                             {'name':'Ready','kind':'constructor','schema':[eq,proof]}],
          'initial':[{'table':'Node','values':[{'sort':'N','eq':'n'}],'subsumed':False}],
          'rulesets':[{'name':'r','rules':[{'name':'base','body':[{'call':{'table':'Node'},'args':[{'var':'x','sort':eq}]}],
             'head':[{'op':'let','dst':'p','sort':proof,'call':{'table':'Ready'},'args':[{'var':'x','sort':eq}]}]}]}]}

    def test_reject_union(self):
        self.core['rulesets'][0]['rules'][0]['head'].append({'op':'union','args':[]})
        with self.assertRaisesRegex(ValueError,'unary constructor'):compile_context(self.core,'r')

    def test_reject_function_updates(self):
        self.core['tables'][1]['kind']='function'
        with self.assertRaisesRegex(ValueError,'unary equality'):compile_context(self.core,'r')

    def test_reject_two_anchor_join(self):
        r=self.core['rulesets'][0]['rules'][0];r['body'].append(copy.deepcopy(r['body'][0]))
        with self.assertRaisesRegex(ValueError,'one immutable'):compile_context(self.core,'r')

    def test_reject_hidden_output(self):
        self.core['initial'].append({'table':'Ready','values':[{'sort':'N','eq':'n'},{'sort':'Proof','eq':'p'}],'subsumed':True})
        with self.assertRaisesRegex(ValueError,'visibility'):compile_context(self.core,'r')

    def test_accept_empty_premise_base(self):
        p=compile_context(self.core,'r')
        self.assertEqual(p['initial'],[0]);self.assertEqual(p['clauses'],1)

if __name__=='__main__':unittest.main()
