import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/bench'))
from math_graph_equal import verify


class TypedEquality(unittest.TestCase):
    def test_sort_scoped_ids_and_result_sort_mismatch(self):
        left=[dict(op='A',sort='A',args=[],result=0),dict(op='B',sort='B',args=[],result=0),
              dict(op='C',sort='C',args=[['A',0],['B',0]],result=0)]
        right=[dict(op='A',sort='A',args=[],result=7),dict(op='B',sort='B',args=[],result=9),
               dict(op='C',sort='C',args=[['A',7],['B',9]],result=8)]
        with tempfile.TemporaryDirectory() as d:
            a,b=Path(d)/'a.json',Path(d)/'b.json';a.write_text(json.dumps(left));b.write_text(json.dumps(right))
            result=verify(a,b,['A','B','C'])
            self.assertEqual(result['eclasses'],3)
            right[0]['sort']='B';b.write_text(json.dumps(right))
            with self.assertRaisesRegex(AssertionError,'sort changed'):verify(a,b,['A','B','C'])


class NativeIf(unittest.TestCase):
    def test_real_rules_on_fresh_matching_and_nonmatching_contexts(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='if-cell-test-') as d:
            out=Path(d)/'run'
            subprocess.run([sys.executable,ROOT/'tools/bench/eggcc_if.py','--output',out,'--ifs','10',
                            '--skip-build','--no-pristine'],cwd=ROOT,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            report=json.loads((out/'report.json').read_text())
            self.assertEqual(report['training']['families'],3)
            for c in report['cases']:
                self.assertTrue(c['verification']['exact_equal'])
                self.assertEqual(len(c['verification']['sorts']),6)
                self.assertEqual(c['shapes']['original_before_export']['enodes'],0)
                for m in c['timings'].values():
                    self.assertIsNone(m['result']['verification'])
                    self.assertFalse(m['result']['traced'])
                if c['distribution']=='sparse':
                    self.assertEqual(c['kinds'],{'min':1,'max':0,'miss':9})
                    self.assertEqual(c['verification']['enodes'],169)
            # The consumer must project Get(Single(x),0) from the parent field;
            # requiring that hidden physical row made this source fail before.
            validation=json.loads((out/'bake/validation.json').read_text())
            self.assertTrue(all(v['comparison']['status']=='equivalent' for v in validation['cases']))


if __name__=='__main__':unittest.main()
