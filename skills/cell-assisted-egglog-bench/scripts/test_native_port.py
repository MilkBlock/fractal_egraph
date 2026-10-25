"""Fast cache and timer-accounting checks; no workload execution."""
import json
from pathlib import Path
import tempfile
import unittest
import native_port as N


class NativePortTests(unittest.TestCase):
    def test_cache_requires_success_and_intact_files(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d); identity={'source':'a','binary':'b'}
            self.assertFalse(N.completed(p,identity))
            for n in ['timing.json','runtime.json']: (p/n).write_text('{}')
            r=dict(identity=identity,returncode=0,files={n:N.sha(p/n) for n in ['timing.json','runtime.json']})
            N.write(p/'lane.json',r)
            self.assertTrue(N.completed(p,identity))
            self.assertFalse(N.completed(p,{'source':'changed','binary':'b'}))
            r['returncode']=1;N.write(p/'lane.json',r)
            self.assertFalse(N.completed(p,identity))
            r['returncode']=0;N.write(p/'lane.json',r)
            (p/'runtime.json').write_text('{"changed":true}')
            self.assertFalse(N.completed(p,identity))

    def test_breakdown_keeps_rhs_in_search_and_separates_rebuild(self):
        r={k+'_time_per_ruleset':{} for k in ['search_and_apply','merge','rebuild']}
        r['search_and_apply_time_per_ruleset']['keys']={'secs':2,'nanos':500000000}
        r['merge_time_per_ruleset']['keys']={'secs':1,'nanos':0}
        r['rebuild_time_per_ruleset']['keys']={'secs':0,'nanos':100000000}
        self.assertEqual(N.native_breakdown(r),[dict(ruleset='keys',search_and_apply=2.5,merge=1.0,rebuild=.1,total=3.6)])
