import tempfile
import unittest
from pathlib import Path
import native_suite as S

class SuiteTests(unittest.TestCase):
    def test_missing_cases_and_failures_remain_in_denominator(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)
            S.write(p/'manifest.json',{'workloads':[{'id':'a'},{'id':'b'},{'id':'pending'}]})
            def lane(n):
                counts={k:n for k in ['constructor_rows','visible_constructor_rows','constructor_eclasses','expr_enodes','expr_eclasses','auxiliary_rows']}
                counts['measurement_seconds']=.01
                phase={'native':{'source_cache':'same-input','execution_seconds':1.,'counts':counts,'runtime':{}},'serialization_seconds':.1,'extraction_seconds':.2}
                return {'returncode':0,'wall_seconds':2.,'passes':{'1':phase}}
            S.write(p/'cases/a/result.json',{'source':'a','before':lane(10),'after':lane(12),'same_final_ir':False})
            S.write(p/'cases/b/result.json',{'source':'b','before':{'returncode':1,'passes':{}}})
            r=S.summarize(p)
            self.assertEqual((r['requested'],r['completed'],r['paired_success']),(3,2,1))
            self.assertEqual(r['passes'][0]['constructor_rows_delta'],2)
            self.assertEqual(r['passes'][0]['constructor_rows_ratio'],1.2)
            self.assertTrue(r['passes'][0]['same_pass_input'])
            self.assertEqual(r['identical_final_ir'],0)
            self.assertIn('b', (p/'workloads.csv').read_text())
