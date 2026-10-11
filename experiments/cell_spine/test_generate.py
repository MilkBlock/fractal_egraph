import unittest
from generate import generate


class GenerateTests(unittest.TestCase):
    def test_declares_summary_before_use(self):
        text = generate(7)
        for n in range(3, 7):
            declaration = f'(relation Summary-Add{n+1} (Spine))'
            action = f'((Summary-Add{n+1} (Spine-Add s (Input x))))'
            self.assertLess(text.index(declaration), text.index(action))

    def test_finite_whitelist(self):
        text = generate(7)
        self.assertIn('Summary-Add7', text)
        self.assertNotIn('Summary-Add8', text)
        self.assertEqual(text.count(':ruleset cell-transition)'), 4)

    def test_invalid_domain(self):
        with self.assertRaises(ValueError):
            generate(2)


if __name__ == '__main__':
    unittest.main()
