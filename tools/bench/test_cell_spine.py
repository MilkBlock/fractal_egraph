"""Small regression for fresh bindings, untimed verification, and phase accounting."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]


class FreshGraphs(unittest.TestCase):
    def test_frozen_library_on_two_unseen_instances(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='spine-bench-test-') as temp:
            out=Path(temp)/'run'
            subprocess.run([sys.executable,ROOT/'tools/bench/cell_spine.py','--output',out,
                            '--copies','2','--skip-build','--no-pristine'],cwd=ROOT,check=True,
                           stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            report=json.loads((out/'report.json').read_text())
            self.assertEqual(len(report['training']),2)
            self.assertEqual(len(report['cases']),2)
            for case in report['cases']:
                self.assertEqual(case['verification'],{'exact_equal':True,'enodes':26,'eclasses':14})
                self.assertEqual(case['shapes']['original_before_export']['enodes'],0)
                self.assertGreater(case['cellify_seconds'],0)
                for mode,m in case['timings'].items():
                    r=m['result']
                    self.assertIsNone(r['verification'])
                    self.assertFalse(r['traced'])
                    self.assertAlmostEqual(r['seconds'],sum(r[k] for k in ('setup_seconds','input_seconds','run_seconds','export_seconds')))
                    self.assertGreater(m['peak_rss_bytes'],0)
                    self.assertEqual(r['export_seconds']==0,mode!='spine_export')
                source=(out/f'{case["workload"]}-2/source.egg').read_text()
                self.assertIn('heldout-0/x',source)
                self.assertIn('heldout-1/z',source)
            self.assertTrue((out/'index.html').is_file())


if __name__=='__main__':unittest.main()
