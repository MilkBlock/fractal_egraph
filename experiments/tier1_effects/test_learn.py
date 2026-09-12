import json
from pathlib import Path
import unittest
from learn import encode

class DagEncodingTests(unittest.TestCase):
    def test_multiple_roots_share_one_definition_table(self):
        r=json.loads(Path(__file__).with_name('results.json').read_text())
        roots=[i['template'] for i in r['instances'] if i['event']<20]
        p=encode(roots,r['native_egraph'])
        self.assertEqual(len(p['args'][1]['args']),3)  # Empty, R10, R15
        p2=encode([roots[0],roots[0]],r['native_egraph'])
        self.assertEqual(p2['args'][0]['args'][0],p2['args'][0]['args'][1])
        self.assertEqual(len(p2['args'][1]['args']),2)
        self.assertNotIn('Occurrence',json.dumps(p))
    def test_cycle_is_not_silently_unrolled(self):
        g={'nodes':{'n':{'op':'CoarseComb','eclass':'Comb-0','children':['n']}}}
        with self.assertRaisesRegex(ValueError,'cyclic'):
            encode(['Comb-0'],g)

if __name__=='__main__':unittest.main()
