import copy,json,tempfile,unittest
from pathlib import Path
from .passes import PassManager

class PassTests(unittest.TestCase):
    def test_unowned_output_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);f=p/'source.egg';f.write_text('user data')
            with self.assertRaises(ValueError):PassManager(p)
            self.assertEqual(f.read_text(),'user data')
    def test_cache_integrity_failure_and_dependency_invalidation(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);source=root/'input';source.write_text('a');code=root/'code';code.write_text('v1');counts=[]
            def compile(text='ok'):
                pm=PassManager(root/'out')
                def action(f):counts.append(1);(f/'result').write_text(source.read_text()+text)
                def verify(f):
                    if not (f/'result').read_text().endswith('ok'):raise ValueError('bad artifact')
                folder=pm.run('lower',{'source':source},{},[code],['result'],action,verify)
                return pm,folder
            a,folder=compile();self.assertFalse(a.receipts[0]['cached'])
            a,_=compile();self.assertTrue(a.receipts[0]['cached']);self.assertEqual(len(counts),1)
            (folder/'result').write_text('corrupt');a,_=compile();self.assertFalse(a.receipts[0]['cached'])
            source.write_text('b');a,_=compile();self.assertFalse(a.receipts[0]['cached'])
            code.write_text('v2')
            with self.assertRaises(ValueError):compile('broken')
            failure=json.loads((root/'out/pipeline.json').read_text());self.assertEqual(failure['failed']['pass_name'],'lower')
            a,_=compile();self.assertFalse(a.receipts[0]['cached'])

    def test_glob_artifacts_are_verified_as_a_set(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);code=root/'code';code.write_text('v');calls=[]
            def build():
                p=PassManager(root/'out')
                f=p.run('analyze',{}, {},[code],['*.json'],lambda f:(calls.append(1),(f/'a.json').write_text('{}')),lambda f:None)
                return p,f
            _,f=build();(f/'injected.json').write_text('{}');p,_=build()
            self.assertFalse(p.receipts[0]['cached']);self.assertEqual(len(calls),2)

if __name__=='__main__':unittest.main()
