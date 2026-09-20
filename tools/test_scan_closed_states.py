import sys
from pathlib import Path
import tempfile
import unittest
from scan_closed_states import annotate, run_bounded


class ScanTests(unittest.TestCase):
    def test_timeout_only_terminates_the_owned_process_group(self):
        with tempfile.TemporaryDirectory() as temp:
            code, status, elapsed, peak = run_bounded(
                [sys.executable, '-c', 'import time;time.sleep(30)'],
                Path(temp) / 'log', .05, 0)
        self.assertNotEqual(code, 0)
        self.assertEqual(status, 'timeout')
        self.assertLess(elapsed, 5)

    def test_annotation_preserves_canonical_ports_and_checks_rule_identity(self):
        r = {'uses': [{'template': 0}], 'templates': [{'pattern': {'steps': [
            {'schema': 0, 'wiring': [{'Input': 0}], 'aliases': [0]}]}}],
            'schemas': [{'rule': 'R', 'input_roles': ['x'], 'output_roles': ['y']}]}
        result = {'origin': {'comb_members': [{'slot': 0, 'rule': 'R', 'parents': [2, 1]}]}}
        annotate(result, r, 0)
        m = result['origin']['comb_members'][0]
        self.assertEqual(m['parents'], [1, 2])
        self.assertEqual(m['binding'], [{'Input': 0}])
        m['rule'] = 'different'
        with self.assertRaises(ValueError):
            annotate(result, r, 0)


if __name__ == '__main__':
    unittest.main()
