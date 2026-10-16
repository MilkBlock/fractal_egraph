"""Correctness tests, including a hard ban on expansion in the compact path."""
from collections import Counter
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from .demo import CELLS, ROOT, Compile, Family, Graph, matrix_search, run


class FactorizedBoundary(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='factorized-spine-')
        cls.addClassCleanup(cls.temp.cleanup)
        cls.out = Path(cls.temp.name)/'demo'
        cls.report = run(cls.out, ROOT/'target/release/examples/native_math_baseline')

    def test_native_full_closures_and_exact_boundary_bindings(self):
        r = self.report
        self.assertEqual(len(r['closure_checks']), 48)
        self.assertTrue(r['complete_native_term_family'])
        self.assertTrue(r['native_binding_equal'])
        self.assertEqual(r['native_distinct_bindings'], 12)
        self.assertEqual(r['native_graft_checks'], 12)
        self.assertEqual(r['scores'], [[8,4,4,0],[4,8,0,0],[4,0,8,0],[0,0,0,8]])

    def test_compact_computation_never_enumerates(self):
        f = Family()
        roots = [f.cell(*c) for c in CELLS]
        c = Compile(f, Graph(self.out/'parents/graph.json'))
        with patch.object(Family, 'enumerate', side_effect=AssertionError('expanded!')):
            result = matrix_search([c.root_mul(r) for r in roots])
        self.assertEqual(result['scores'], self.report['scores'])

    def test_correlated_binding_is_not_replaced_by_cartesian_product(self):
        result = matrix_search([Counter({('x','u'):2, ('y','v'):3}),
                                Counter({('x','w'):5})])
        matches = [r for r in result['records'] if (r['left'],r['right']) == (0,1)]
        self.assertEqual([(r['factor'],r['u'],r['v']) for r in matches], [('x','u','w')])
        self.assertEqual(result['scores'][0][1], 10)

    def test_invalid_union_of_choices_rejected(self):
        f = Family()
        invalid = f.node('Or', f.node('Var','x'), f.node('Var','y'))
        c = Compile(f, Graph(self.out/'parents/graph.json'))
        with self.assertRaisesRegex(ValueError, 'not in one native eclass'):
            c.value(invalid)

    def test_repeated_variables_require_a_different_choice_schema(self):
        with self.assertRaisesRegex(ValueError, 'distinct Var'):
            Family().cell('x','a','a')


if __name__ == '__main__':
    unittest.main()
