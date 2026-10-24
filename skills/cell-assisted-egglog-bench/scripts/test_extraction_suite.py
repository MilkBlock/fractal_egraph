import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0,str(Path(__file__).resolve().parent))
import bench
import extraction_suite as suite


class ExtractionSuite(unittest.TestCase):
    def test_measured_wait_never_uses_posix_timeout_polling(self):
        original=suite.subprocess.Popen.wait
        calls=[]
        def checked_wait(process,*args,**kwargs):
            calls.append((args,kwargs))
            self.assertFalse(args)
            self.assertNotIn('timeout',kwargs)
            return original(process)
        with tempfile.TemporaryDirectory() as tmp,mock.patch.object(suite.subprocess.Popen,'wait',checked_wait):
            result=suite.run_process([sys.executable,'-c','print("done")'],Path(tmp)/'run',timeout=5)
        self.assertEqual(result['returncode'],0)
        self.assertTrue(calls)

    def test_dry_run_has_no_build_or_output_side_effect(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)
            rc=bench.main(['--dry-run','extraction-suite','--checkout',str(p/'missing'),
                          '--build-dir',str(p/'build'),'--output',str(p/'result')])
            self.assertEqual(rc,0)
            self.assertEqual(list(p.iterdir()),[])

    def test_cache_rechecks_input_and_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);source=p/'input';source.write_text('fixed input')
            command=[sys.executable,'-c','import sys; print(sys.stdin.read())']
            first=suite.cached_process(command,p/'run',stdin=source,env=suite.clean_env(),timeout=5)
            self.assertEqual(first['returncode'],0)
            self.assertTrue(suite.cached_process(command,p/'run',stdin=source,env=suite.clean_env(),timeout=5)['reused'])
            source.write_text('changed input')
            with self.assertRaises(ValueError):suite.cached_process(command,p/'run',stdin=source,env=suite.clean_env(),timeout=5)

    def test_failures_remain_in_denominator_without_fabricated_speedups(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);suite.write(p/'manifest.json',dict(inventory={'a':'x','b':'y'}))
            def graph(passed=True):
                return dict(quality_passed=passed,graph=dict(serialized_nodes=7,serialized_eclasses=5),
                            native_estimate_seconds=1.,bag_estimate_seconds=.25,
                            runs={'native':dict(wall_seconds=2.,max_rss_kib=1024,stats=[]),
                                  'bag':dict(wall_seconds=1.,max_rss_kib=512,stats=[dict(merges=2)])})
            for name,passed in [('a',True),('b',False)]:
                d=p/'cases'/name;d.mkdir(parents=True)
                suite.write(d/'result.json',dict(name=name,suite='bril',source=name,completed=True,
                    complete_native_pipeline=passed,all_frozen_checks_passed=passed,
                    graphs=[graph(passed) for _ in range(3)],pipeline_speedup=None,
                    pipeline_output_equal=passed,intermediate_graphs_byte_equal=False,
                    pipeline={'native':dict(wall_seconds=10.,returncode=0 if passed else 1,timeout=False),
                              'bag':dict(wall_seconds=9.,returncode=0,timeout=False)}))
            summary=suite.summarize(p)
            self.assertEqual((summary['inventory'],summary['attempted'],summary['frozen_checked_programs']),(2,2,1))
            self.assertEqual(summary['extraction_geomean'],2.)
            self.assertIsNone(summary['pipeline_geomean'])
            self.assertIsNone(summary['rows'][1]['extraction_speedup'])
            self.assertEqual(summary['rows'][0]['rss_ratio'],2.)


if __name__=='__main__':unittest.main()
