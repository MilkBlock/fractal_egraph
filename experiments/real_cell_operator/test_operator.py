"""Unit checks for exact scalar operators; no native run or Cell fixtures.

Run with: python3 -m unittest discover -s experiments/real_cell_operator -p 'test_*.py'
"""

from fractions import Fraction
from types import SimpleNamespace
import unittest

from algebra import coordinates, interface_basis, shared_affine_program
from derive import Affine, extract_equations, multiply, plus, scalar_subexpressions, scale


def constant(value):
    return ("const", value)


def field(value):
    return ("field", value)


def operation(name, left, right):
    return (name, (left, right))


def evaluate(form, inputs):
    return sum(coefficient * (1 if key is None else inputs[key])
               for key, coefficient in form.items())


class AffineTests(unittest.TestCase):
    def test_zero_coefficient_does_not_erase_input_presence_obligation(self):
        expression = operation("*", constant(0), field(7))
        self.assertEqual(Affine({}, {}).expr(expression), {})
        self.assertIn(field(7), list(scalar_subexpressions(expression)))

    def test_nested_intermediate_is_retained_when_final_offset_cancels(self):
        intermediate = operation("+", field(0), constant(10))
        expression = operation("-", intermediate, constant(9))
        parts = list(scalar_subexpressions(expression))
        self.assertIn(expression, parts)
        self.assertIn(intermediate, parts)
        self.assertIn(field(0), parts)
        self.assertIn(constant(10), parts)
        self.assertIn(constant(9), parts)
        # The final x+1 fits while the actual x+10 intermediate overflows i64.
        value = (1 << 63) - 3
        affine = Affine({}, {})
        self.assertLessEqual(evaluate(affine.expr(expression), {0: value}), (1 << 63) - 1)
        self.assertGreater(evaluate(affine.expr(intermediate), {0: value}), (1 << 63) - 1)

    def test_repeated_boundary_is_counted_with_multiplicity(self):
        equations = {
            1: [operation("-", operation("*", constant(3), field(0)), constant(2))],
            2: [operation("+", operation("+", field(1), field(0)), constant(5))],
        }
        form = Affine(equations, {}).field(2)
        self.assertEqual(form, {0: 4, None: 3})
        for value in (-7, 0, 1, 23, 2**54 + 1):
            self.assertEqual(evaluate(form, {0: value}), (3 * value - 2) + value + 5)

    def test_constant_scaling_on_either_side_and_cancellation(self):
        affine = Affine({}, {})
        for left, right in ((constant(-3), field(7)), (field(7), constant(-3))):
            self.assertEqual(affine.expr(operation("*", left, right)), {7: -3})
        zero = operation("-", field(7), field(7))
        self.assertEqual(affine.expr(operation("*", zero, field(8))), {})
        self.assertEqual(plus({7: 2, None: 5}, {7: -2, None: -5}), {})
        self.assertEqual(scale(0, {7: 2, None: 5}), {})

    def test_cut_preserves_parent_coordinate_instead_of_baked_seed(self):
        equations = {
            1: [operation("+", field(0), constant(2))],
            2: [operation("+", field(1), constant(1))],
        }
        self.assertEqual(Affine(equations, {}, cuts=(1,)).field(2), {1: 1, None: 1})
        self.assertEqual(Affine(equations, {}).field(2), {0: 1, None: 3})
        self.assertEqual(Affine({}, {1: 99}, cuts=(1,)).field(1), {1: 1})

    def test_constant_merge_uses_min_and_equal_forms_are_accepted(self):
        self.assertEqual(Affine({1: [constant(9), constant(3)]}, {1: 7}).field(1), {None: 3})
        same = operation("+", field(0), constant(2))
        self.assertEqual(Affine({1: [same, same]}, {}).field(1), {0: 1, None: 2})
        affine = Affine({}, {})
        self.assertEqual(affine.expr(operation("min", constant(8), constant(-2))), {None: -2})
        self.assertEqual(affine.expr(operation("max", same, same)), {0: 1, None: 2})

    def test_nonlinear_product_is_rejected(self):
        for expression in (
            operation("*", field(0), field(1)),
            operation("*", field(0), field(0)),
        ):
            with self.subTest(expression=expression):
                with self.assertRaisesRegex(ValueError, "nonlinear multiplication"):
                    Affine({}, {}).expr(expression)

    def test_actual_piecewise_min_and_max_are_rejected(self):
        # x + 1 and 2*x cross at x=1, including on a nonnegative domain.
        left = operation("+", field(0), constant(1))
        right = operation("*", constant(2), field(0))
        for operator in ("min", "max"):
            with self.subTest(operator=operator):
                with self.assertRaisesRegex(ValueError, "nontrivial min/max"):
                    Affine({}, {}).expr(operation(operator, left, right))
        with self.assertRaisesRegex(ValueError, "min envelope"):
            Affine({1: [left, right]}, {}).field(1)

    def test_baked_numeric_seed_does_not_certify_symbolic_min(self):
        with self.assertRaisesRegex(ValueError, "min envelope"):
            Affine({1: [field(0)]}, {1: 5}).field(1)

    def test_direct_and_indirect_cycles_are_rejected(self):
        for equations in ({0: [field(0)]}, {0: [field(1)], 1: [field(0)]}):
            with self.subTest(equations=equations):
                with self.assertRaisesRegex(ValueError, "recursive scalar equations"):
                    Affine(equations, {}).field(0)
        # An explicit interface cut terminates this dependency by contract.
        self.assertEqual(Affine({0: [field(1)], 1: [field(0)]}, {}, cuts=(1,)).field(0), {1: 1})


class BasisTests(unittest.TestCase):
    def test_shifted_outputs_reconstruct_with_nonboolean_offsets(self):
        rows = [[1, 2], [1, 4], [1, 5]]
        result = interface_basis(rows)
        self.assertEqual(result["owners"], ["constant", 0])
        self.assertEqual(result["rank"], 2)
        self.assertEqual(result["transforms"], [["0", "1"], ["2", "1"], ["3", "1"]])
        transforms = [[Fraction(value) for value in row] for row in result["transforms"]]
        self.assertEqual(multiply(transforms, result["basis"]), rows)
        for value in (0, 1, 2**54 + 1):
            self.assertEqual(
                multiply(transforms, multiply(result["basis"], [[value], [1]])),
                [[value + 2], [value + 4], [value + 5]],
            )

    def test_rational_reconstruction_is_exact(self):
        rows = [[2, 1], [1, 0], [0, 0]]
        result = interface_basis(rows)
        self.assertEqual(result["transforms"][1], ["-1/2", "1/2"])
        transforms = [[Fraction(value) for value in row] for row in result["transforms"]]
        self.assertEqual(multiply(transforms, result["basis"]), rows)

    def test_coordinates_preserve_integers_above_float_precision(self):
        large = 2**53 + 1
        self.assertEqual(
            coordinates([[large, 1], [0, 1]], [1, 0]),
            [Fraction(1, large), Fraction(-1, large)],
        )

    def test_independent_outputs_and_known_constants(self):
        rows = [[0, 0, 7], [1, 0, 2], [0, 1, 3], [1, 1, 5], [0, 0, 0]]
        result = interface_basis(rows)
        self.assertEqual(result["owners"], ["constant", 1, 2])
        self.assertEqual(result["rank"], 3)
        transforms = [[Fraction(value) for value in row] for row in result["transforms"]]
        self.assertEqual(multiply(transforms, result["basis"]), rows)
        self.assertIsNone(coordinates([[1, 0, 0]], [0, 1, 0]))
        self.assertEqual(coordinates([], [0, 0]), [])
        self.assertIsNone(coordinates([], [0, 1]))


class CompositionTests(unittest.TestCase):
    def test_malformed_dimensions_are_rejected_before_composition(self):
        for left, right in (
            ([[1, 2]], [[3]]),
            ([[1]], [[2], [3]]),
            ([[1, 2], [3]], [[1], [2]]),
            ([[1, 2]], [[1, 2], [3]]),
        ):
            with self.subTest(left=left, right=right):
                with self.assertRaises(ValueError):
                    multiply(left, right)

    def test_parent_matrix_keeps_accumulated_arithmetic_constants(self):
        # Parent outputs are x+2 and constant1; a binary node adds another1.
        parents = [[1, 2], [0, 1], [0, 1]]
        binary = multiply([[1, 1, 1]], parents)
        self.assertEqual(binary, [[1, 4]])
        unary = multiply([[1, 1]], [binary[0], [0, 1]])
        self.assertEqual(unary, [[1, 5]])
        self.assertEqual(multiply(unary, [[1], [1]]), [[6]])
        self.assertNotEqual(multiply([[1, 1]], [[1], [1]]), [[6]])

    def test_rectangular_composition_is_associative_over_rationals(self):
        arithmetic = [[2, -1, Fraction(1, 2)]]
        parents = [[1, 3], [2, 5], [0, 1]]
        inputs = [[7, 11], [1, 1]]
        self.assertEqual(
            multiply(multiply(arithmetic, parents), inputs),
            multiply(arithmetic, multiply(parents, inputs)),
        )
        self.assertEqual(multiply(arithmetic, parents), [[0, Fraction(3, 2)]])


class SharedProgramTests(unittest.TestCase):
    def test_shifted_outputs_share_one_emitted_basis_computation(self):
        lets, outputs, basis = shared_affine_program([[1, 2], [1, 4], [1, 5]])
        self.assertEqual(lets, ["(let cell_basis_1 (+ cell_size 2))"])
        self.assertEqual(outputs, ["cell_basis_1", "(+ 2 cell_basis_1)", "(+ 3 cell_basis_1)"])
        self.assertEqual(basis["owners"], ["constant", 0])
        self.assertEqual(basis["rank"], 2)
        self.assertTrue(all("cell_size" not in expression for expression in outputs))

    def test_nonboolean_coefficient_is_preserved_in_native_expression(self):
        lets, outputs, _ = shared_affine_program([[2, 1], [4, 3]], input_name="argument_size")
        self.assertEqual(lets, ["(let cell_basis_1 (+ (* 2 argument_size) 1))"])
        self.assertEqual(outputs, ["cell_basis_1", "(+ 1 (* 2 cell_basis_1))"])

    def test_constant_only_outputs_need_no_runtime_basis_let(self):
        lets, outputs, basis = shared_affine_program([[0, 0], [0, 4]])
        self.assertEqual(lets, [])
        self.assertEqual(outputs, ["0", "4"])
        self.assertEqual(basis["owners"], ["constant"])

    def test_rational_and_negative_reconstructions_are_rejected(self):
        for label, rows in (
            ("fractional transform", [[2, 0], [1, 0]]),
            ("negative transform", [[1, 5], [1, 2]]),
            ("fractional basis", [[Fraction(1, 2), 0]]),
            ("negative basis", [[-1, 2]]),
            ("unrepresentable i64 coefficient", [[1 << 63, 0]]),
        ):
            with self.subTest(case=label):
                with self.assertRaisesRegex(ValueError, "integer nonnegative reconstruction"):
                    shared_affine_program(rows)

    def test_multiple_boundaries_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "one scalar boundary"):
            shared_affine_program([[1, 2, 3]])


class ExtractionRejectionTests(unittest.TestCase):
    def test_scalar_equality_cannot_overwrite_existing_structural_binding(self):
        variable = lambda name: {"kind": "var", "name": name}
        call = lambda head, *args: {"kind": "call", "head": head, "args": list(args)}
        # A structural table match has already bound sum to the i64 value9.
        # The subsequent equality must constrain that value, not define a local.
        graph = SimpleNamespace(
            values=[{"sort": "Expr", "literal": None}, {"sort": "i64", "literal": "9"}],
            by_op={"Expr-size": []},
            query=lambda _: [{"expr": 0, "sum": 1}],
            resolve=lambda expression, env: env[expression["name"]],
        )
        rule = {
            "ordinal": 0,
            "ruleset": "size",
            "rule_text": "bound scalar equality unit case",
            "body": [
                {"kind": "fact", "expr": call("Ready", variable("expr"), variable("sum"))},
                {"kind": "eq", "args": [variable("sum"), call("Expr-size", variable("expr"))]},
            ],
            "head": [{"kind": "set", "head": "Expr-size", "args": [variable("expr")],
                      "value": variable("sum")}],
        }
        ast = {
            "signatures": {"Expr-size": {"merge": call("min", variable("old"), variable("new"))}},
            "rules": [rule],
        }
        with self.assertRaises(ValueError):
            extract_equations(graph, ast, ["size"])


if __name__ == "__main__":
    unittest.main()
