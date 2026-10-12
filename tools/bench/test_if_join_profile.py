"""Attribution units and the opt-in profiler's native semantic parity."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parent))
from if_join_profile import ROOT,RUNNER,blocks
from math_graph_equal import verify


def line(kind,enum,lookup,probes,entries='1'):
    stages=1 if kind!='plan' else 0
    materialized='2\t2\t1\t0' if kind!='plan' else '-\t-\t-\t-'
    return (f'EGGLOG_JOIN_WORK\t{"rule λ".encode().hex()}\t{kind}\t0\t{stages}\t100\t{materialized}'
            f'\tcandidate_rows={enum+lookup}\t  candidate_rows enumerated (for_each)={enum}'
            f'\t  candidate_rows from lookups={lookup}\tprobes={probes}\tstage_entries={entries if stages else ""}')


class ProfileParsing(unittest.TestCase):
    def test_utf8_names_and_block_plan_accounting(self):
        result=blocks(line('materialize',3,4,2)+'\n'+line('plan',3,4,2))
        self.assertEqual(result[0]['rule'],'rule λ')
        self.assertEqual(result[0]['materialized_rows'],2)
        self.assertEqual(result[0]['counters']['candidate_rows'],7)
        self.assertEqual(result[0]['stage_entries'],[1])

    def test_rejects_incomplete_trace_and_later_counter_overflow(self):
        with self.assertRaisesRegex(ValueError,'incomplete'):blocks(line('materialize',3,4,2))
        # The final counter also has to be checked, not just candidate_rows.
        with self.assertRaisesRegex(ValueError,'exceeds'):
            blocks(line('materialize',3,4,9)+'\n'+line('plan',3,4,2))


@unittest.skipUnless(os.getenv('EGG_LAYOUT_TEST_JOIN_PROFILE')=='1','requires native-join-work-profile build')
class NativeProfile(unittest.TestCase):
    def test_profile_is_opt_in_and_preserves_full_graph(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='join-profile-test-') as temp:
            d=Path(temp)
            (d/'env.egg').write_text('''(datatype T (A i64) (B T))
(ruleset r)
(rule ((A x)) ((B (A x))) :ruleset r :name "step")
''')
            (d/'input.egg').write_text('(A 7)')
            (d/'run.json').write_text(json.dumps(dict(environment=str(d/'env.egg'),input=str(d/'input.egg'),
                run='(run-schedule (saturate r))',constructors=['A','B'])))
            for enabled in (False,True):
                env=dict(os.environ,RAYON_NUM_THREADS='1');env.pop('EGGLOG_JOIN_WORK_RULES',None)
                if enabled:env['EGGLOG_JOIN_WORK_RULES']='step'
                graph=d/f'{enabled}.json'
                r=subprocess.run([RUNNER,'bench-plans',d/'run.json',graph],cwd=ROOT,env=env,text=True,capture_output=True,check=True)
                data=json.loads(r.stdout.splitlines()[-1])
                self.assertEqual(data['matches'],1)
                self.assertIn('run_report',data['verification'])
                result=blocks(r.stderr)
                if enabled:
                    self.assertTrue(result,'rebuild with --features native-join-work-profile')
                    self.assertTrue(any(b['kind']=='single' and sum(b['stage_entries'])>0 for b in result))
                else:self.assertFalse(result)
            self.assertEqual(verify(d/'False.json',d/'True.json',['T'])['enodes'],2)
            env=dict(os.environ,RAYON_NUM_THREADS='2',EGGLOG_JOIN_WORK_RULES='step')
            parallel=subprocess.run([RUNNER,'bench-plans',d/'run.json',d/'parallel.json'],cwd=ROOT,env=env,text=True,capture_output=True)
            self.assertNotEqual(parallel.returncode,0)
            self.assertIn('requires one thread',parallel.stderr)


if __name__=='__main__':unittest.main()
