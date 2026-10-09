"""Focused semantic/algebra tests; no native interpolation generates an operator."""

import itertools
import unittest

from polynomial import ONE, ZERO, Poly, coefficient_matrix, or_delta


class PolynomialTests(unittest.TestCase):
    def setUp(self):
        self.p, self.u, self.v = (Poly.var(i) for i in range(3))

    def test_signed_or_is_membership_not_witness_count(self):
        p, u = self.p, self.u
        union = p | u
        self.assertEqual(union.terms, {1: 1, 2: 1, 3: -1})
        self.assertEqual((p + u).evaluate(3), 2)
        self.assertEqual(union.evaluate(3), 1)
        self.assertEqual(union.to_string(["p", "u"]), "p + u - p*u")

    def test_square_free_product_and_exact_cancellation(self):
        p, u = self.p, self.u
        self.assertEqual(p * p, p)
        self.assertEqual((p + u) ** 2, p + u + 2 * p * u)
        self.assertEqual((p | u) | p, p | u)
        self.assertEqual((p | u) * (p | u), p | u)
        self.assertEqual((p | u) - p - u + p * u, ZERO)
        self.assertEqual((10**80) * p - (10**80) * p, ZERO)

    def test_boolean_laws_and_exhaustive_gate_evaluation(self):
        p, u, v = self.p, self.u, self.v
        self.assertEqual((p | u) | v, p | (u | v))
        self.assertEqual(p * (u | v), (p * u) | (p * v))
        self.assertEqual(p | (p * u), p)
        self.assertEqual(~(p | u), ~p * ~u)
        self.assertEqual(p | ~p, ONE)
        self.assertEqual(p * ~p, ZERO)
        expr = p | (u * v)
        for mask in range(8):
            expected = int(bool(mask & 1) or (bool(mask & 2) and bool(mask & 4)))
            self.assertEqual(expr.evaluate(mask), expected)
        self.assertTrue(expr.is_boolean())
        self.assertFalse((p + u).is_boolean())

    def test_substitution_is_simultaneous_and_preserves_aliases(self):
        p, u, v = self.p, self.u, self.v
        expr = p | u
        self.assertEqual(expr.substitute({0: v, 1: v}), v)
        self.assertEqual((p - u).substitute({0: u, 1: p}), u - p)
        replacements = {0: u | v, 1: p * v}
        composed = expr.substitute(replacements)
        for mask in range(8):
            parent_world = replacements[0].evaluate(mask) | (replacements[1].evaluate(mask) << 1)
            self.assertEqual(composed.evaluate(mask), expr.evaluate(parent_world))
        with self.assertRaises(ValueError):
            expr.substitute({0: p + u})

    def test_composition_associativity(self):
        p, u, v = self.p, self.u, self.v
        output = p | (u * v)
        first = {0: p | u, 1: u * v, 2: ~p}
        second = {0: p * v, 1: p | v, 2: u}
        lhs = output.substitute(first).substitute(second)
        rhs = output.substitute({i: f.substitute(second) for i, f in first.items()})
        self.assertEqual(lhs, rhs)

    def test_complete_finite_difference_retains_cross_term(self):
        p, u, v = self.p, self.u, self.v
        self.assertEqual((p | u).difference(0), 1 - u)
        # Both inputs appear simultaneously: dp=dq=p. Dropping -dp*dq
        # predicts 2*p while exact OR activation is p.
        self.assertEqual(or_delta(ZERO, ZERO, p, p), p)
        self.assertNotEqual(or_delta(ZERO, ZERO, p, p), p + p)
        for left, right in itertools.product([ZERO, ONE, p, p | u, ~v], repeat=2):
            for index in range(3):
                p0, q0 = left.restrict(index, 0), right.restrict(index, 0)
                dp, dq = left.difference(index), right.difference(index)
                self.assertEqual(or_delta(p0, q0, dp, dq), (left | right).difference(index))

    def test_restriction_collects_duplicates(self):
        p, u = self.p, self.u
        self.assertEqual((p + u - p * u).restrict(0, 1), ONE)
        self.assertEqual((p + u - p * u).restrict(0, 0), u)
        self.assertEqual((p - p * u).restrict(1, 1), ZERO)

    def test_matrix_never_discards_a_nonzero_coordinate(self):
        p, u = self.p, self.u
        columns, rows = coefficient_matrix([p, p | u, ONE])
        self.assertEqual(columns, [0, 1, 2, 3])
        self.assertEqual(rows, [[0, 1, 0, 0], [0, 1, 1, -1], [1, 0, 0, 0]])
        with self.assertRaises(ValueError):
            (p | u).coefficient_row([1, 2])
        with self.assertRaises(ValueError):
            p.coefficient_row([1, 1])

    def test_serialization_immutability_and_input_validation(self):
        expr = self.p | self.u
        self.assertEqual(Poly.from_json(expr.to_json()), expr)
        changed = expr.terms
        changed[3] = 40
        self.assertEqual(expr.terms[3], -1)
        with self.assertRaises(AttributeError):
            expr._items = ()
        self.assertEqual(expr.evaluate([False, True]), 1)
        self.assertEqual(expr.evaluate({0: 1, 1: 0}), 1)
        for bad in [-1, {0: 1}, {0: 1, 1: 2}]:
            with self.assertRaises(ValueError):
                expr.evaluate(bad)
        with self.assertRaises(TypeError):
            Poly({1: 0.5})
        with self.assertRaises(ValueError):
            Poly({-1: 1})
        with self.assertRaises(ValueError):
            Poly.from_json({"schema": "square-free-integer-polynomial/v1",
                            "terms": [{"mask": 1, "coefficient": 1},
                                      {"mask": 1, "coefficient": -1}]})


if __name__ == "__main__":
    unittest.main()
