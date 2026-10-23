import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
import json

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE))
import bench
import extraction_markers as em


class ExtractionMarkers(unittest.TestCase):
    def test_cache_requires_exact_inputs_checked_outputs_and_mode(self):
        with tempfile.TemporaryDirectory() as d:
            repo=Path(d);run=repo/'out/previous';run.mkdir(parents=True)
            (run/'bag.egg').write_text('chosen DAG');(run/'bag.stderr').write_text('')
            identity=dict(schema='tiger-extraction-markers/v1',input_sha256='input',tiger_sha256='binary',machine='same')
            packet=dict(identity,quality=dict(passed=True),runs=[dict(mode='bag',returncode=0,timing_eligible=True,output_sha256=em.sha(run/'bag.egg'))])
            (run/'results.json').write_text(json.dumps(packet))
            self.assertIsNotNone(em.cached_lane(repo,identity,'bag'))
            self.assertIsNone(em.cached_lane(repo,dict(identity,input_sha256='changed'),'bag'))
            self.assertIsNone(em.cached_lane(repo,identity,'cpu'))
            (run/'bag.egg').write_text('changed output')
            self.assertIsNone(em.cached_lane(repo,identity,'bag'))

    def test_existing_extract_command_has_explicit_backend(self):
        a=bench.parser().parse_args(['extract','out/saved','--extraction-markers','gpu'])
        command=bench.commands(a,bench.REPO,bench.REPO/'worker',bench.REPO/'out/saved')[0]
        self.assertEqual(command[-2:],['--markers','gpu'])

    def test_existing_run_defaults_to_original_tiger(self):
        a=bench.parser().parse_args(['run','--output','out/new'])
        command=bench.commands(a,bench.REPO,bench.REPO/'worker',bench.REPO/'out/new')[-1]
        self.assertEqual(command[-2:],['--markers','off'])

    def test_benchmark_dry_run_does_not_create_output(self):
        with tempfile.TemporaryDirectory() as d:
            output=Path(d)/'not-created'
            result=bench.main(['--dry-run','extraction-markers','--graph','out/missing.json',
                              '--output',str(output),'--build-dir',str(Path(d)/'build')])
            self.assertEqual(result,0);self.assertFalse(output.exists());self.assertFalse((Path(d)/'build').exists())


if __name__=='__main__':unittest.main()
