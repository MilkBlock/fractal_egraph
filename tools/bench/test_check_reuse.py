import copy
import contextlib
import io
import unittest
from check_reuse import compare

class RegressionCheck(unittest.TestCase):
    def setUp(self):
        self.base = dict(schema='reuse-bench/v1', arch='aarch64', os='macos', scope='test',
                         benchmarks={'a': dict(median_ns=100, batch=128)})

    def check(self, current):
        with contextlib.redirect_stdout(io.StringIO()):
            return compare(self.base, current, .20)

    def test_unchanged(self):
        self.assertEqual(self.check(self.base), [])

    def test_regression(self):
        new = copy.deepcopy(self.base)
        new['benchmarks']['a']['median_ns'] = 121
        self.assertEqual(self.check(new), ['a'])

    def test_missing_benchmark(self):
        new = copy.deepcopy(self.base)
        new['benchmarks'] = {}
        with self.assertRaises(ValueError): self.check(new)

    def test_different_machine_arch(self):
        new = dict(self.base, arch='x86_64')
        with self.assertRaises(ValueError): self.check(new)

    def test_invalid_timing(self):
        for value in (0, -1, float('nan'), float('inf')):
            new = copy.deepcopy(self.base)
            new['benchmarks']['a']['median_ns'] = value
            with self.assertRaises(ValueError): self.check(new)

if __name__ == '__main__':
    unittest.main()
