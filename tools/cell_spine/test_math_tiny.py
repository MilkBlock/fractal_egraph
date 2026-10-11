"""Native math subset regression; no concrete-stage fallback or timing claim."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from .compiler import ROOT


class TinyMath(unittest.TestCase):
    def test_same_closure_from_factored_and_expanded_inputs(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out', prefix='math-spine-test-') as temp:
            out = Path(temp)/'run'
            subprocess.run([sys.executable, ROOT/'experiments/cell_spine/math_tiny/run.py',
                            '--output', out, '--skip-build'], check=True, cwd=ROOT,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            result = json.loads((out/'results.json').read_text())
            self.assertEqual(result['same_final_closure'], 'equivalent')
            for case in result['cases'].values():
                v = case['validation']
                self.assertEqual(v['comparison']['status'], 'equivalent')
                self.assertEqual(v['original_constructor_rows'], 13)
                self.assertEqual(v['restored_constructor_rows'], 13)
                self.assertEqual(v['original_constructor_eclasses'], 7)
                self.assertEqual(v['restored_constructor_eclasses'], 7)
                self.assertEqual(v['physical_original_rows_before_export'], 0)
                self.assertLess(v['spine_constructor_rows'], 13)
                self.assertEqual(v['summary_rows'], sum(v['summary_tables'].values()))
                self.assertTrue(v['compressed_logical_matches_before_export'])
                self.assertTrue((out/'original.svg').exists())
            self.assertGreater(result['cases']['expanded']['multi_parent_matches'], 0)
            self.assertIn('<svg', (out/'index.html').read_text())


if __name__ == '__main__':
    unittest.main()
