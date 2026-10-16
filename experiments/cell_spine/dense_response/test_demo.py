"""Density denominators, exact ranks, native context masks and query semantics."""
from fractions import Fraction
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from .demo import run, exact_factor, apply_response, stats, trim
from experiments.cell_spine.factorized_boundary.demo import Graph, mm
from experiments.cell_spine.shared_matching.demo import Matcher


class ExactRank(unittest.TestCase):
    def test_rank_one_and_full_rank(self):
        self.assertEqual(exact_factor([[1]*5 for _ in range(5)])['rank'],1)
        self.assertEqual(exact_factor([[int(i==j) for j in range(4)] for i in range(4)])['rank'],4)

    def test_fractional_factor_is_exact(self):
        m=[[1,1,1],[1,-1,0],[0,2,1]]
        f=exact_factor(m)
        self.assertEqual(f['rank'],2)
        self.assertTrue(any(x.denominator>1 for row in f['R'] for x in row))
        self.assertEqual(mm(f['C'],f['R']),m)

    def test_zero_matrix_and_empty_support(self):
        self.assertEqual(stats([[0,0],[0,0]])['exact_rank'],0)
        self.assertEqual(trim([[0,0],[0,0]]),([],[],[]))
        self.assertEqual(trim([]),([],[],[]))
        self.assertEqual(apply_response(exact_factor([[0,0]]),[[1],[1]],1),[[0]])


class DensityAudit(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(prefix='cell-density-')
        cls.addClassCleanup(cls.temp.cleanup)
        cls.out=Path(cls.temp.name)/'run'
        cls.report=run(cls.out,skip_build=True)

    def test_actual_context_not_invented(self):
        i=self.report['interaction']
        self.assertEqual((i['raw_channels'],i['unique_channels']),(64,32))
        self.assertEqual(i['potential']['nonzeros'],110)
        self.assertEqual(i['existing']['nonzeros'],14)
        self.assertEqual(i['native_existing_matches'],14)
        self.assertTrue(i['native_equal'])
        self.assertEqual(i['synthetic_graft_facts_added'],0)
        source=(self.out/'existing-probe.egg').read_text()
        self.assertNotIn('Graft',source)
        self.assertNotIn('(let graft',source)

    def test_conditional_complete_blocks_are_only_rank_one(self):
        i=self.report['interaction']
        self.assertEqual(len(i['blocks']),15)
        for b in i['blocks']:
            self.assertEqual(b['potential']['density'],1)
            self.assertEqual(b['potential']['exact_rank'],1)
        self.assertEqual(sum(b['potential']['nonzeros'] for b in i['blocks']),110)
        self.assertEqual(i['active_existing']['shape'],[12,12])
        self.assertEqual(i['active_existing']['exact_rank'],12)

    def test_native_projected_responses_and_denominators(self):
        a,b=self.report['responses']
        self.assertEqual(a['raw']['shape'],[70,7])
        self.assertEqual(b['raw']['shape'],[110,11])
        self.assertEqual(a['active']['shape'],[16,6])
        self.assertEqual(b['active']['shape'],[30,10])
        self.assertEqual([o['active']['exact_rank'] for o in (a,b)],[5,8])
        for o in (a,b):
            self.assertTrue(o['native_equal'])
            self.assertEqual(o['raw']['nonzeros'],o['active']['nonzeros'])
            self.assertGreater(o['active']['rank_factor_multiply_adds'],o['active']['dense_multiply_adds'])

    def test_compiled_response_does_not_rematch(self):
        for o in self.report['responses']:
            f=dict(o['factor'])
            f['R']=[[Fraction(v) for v in row] for row in f['R']]
            n=len(o['columns'])
            query=[[int(i%2==0),int(i%3==0),0,1] for i in range(n)]
            with (patch.object(Matcher,'evaluate',side_effect=AssertionError('rematched')),
                  patch.object(Graph,'value',side_effect=AssertionError('constructor query'))):
                result=apply_response(f,query,len(o['matrix']))
            self.assertEqual(result,mm(o['matrix'],query))


if __name__=='__main__':
    unittest.main()
