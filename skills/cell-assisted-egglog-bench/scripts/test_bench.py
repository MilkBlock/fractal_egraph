"""Fast protocol checks; no compilation, GPU benchmark, Bake or production edits."""
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value

HERE = Path(__file__).resolve().parent
B = module(HERE / 'bench.py', 'cell_skill_bench')
REPO = HERE.parents[2]

def fixture():
    return dict(benchmark_finished=True, native=dict(run_seconds=10),
                sorted_commit_cpu=dict(runtime=dict(run_seconds=8)),
                gpu=dict(audit=False, runtime=dict(run_seconds=4, skipped_saturations=0)),
                same_commit_verification=dict(exact_rows_values_equivalence_visibility=True),
                verification=None)

class WorkflowTests(unittest.TestCase):
    def test_speedup_requires_exact_graph_and_unaudited_timing(self):
        r = fixture()
        self.assertEqual(B.metrics(r)['inference_speedup'], 2)
        for proof in [None, {}, dict(rows=100), dict(exact_rows_values_equivalence_visibility=False)]:
            changed = copy.deepcopy(r); changed['same_commit_verification'] = proof
            self.assertIsNone(B.metrics(changed, dict(passed=True))['inference_speedup'])
        for audit in [True, None]:
            changed = copy.deepcopy(r); changed['gpu']['audit'] = audit
            self.assertIsNone(B.metrics(changed)['inference_speedup'])

    def test_extraction_ratio_requires_actual_successful_extraction(self):
        r = fixture(); t = dict(runs=[dict(lane='cpu', seconds=2, returncode=0), dict(lane='gpu', seconds=1, returncode=0)])
        self.assertIsNone(B.metrics(r, None, t)['inference_plus_tiger_speedup'])
        self.assertEqual(B.metrics(r, dict(passed=True), t)['inference_plus_tiger_speedup'], 2)
        t['runs'][1]['returncode'] = 1
        self.assertIsNone(B.metrics(r, dict(passed=True), t)['inference_plus_tiger_speedup'])

    def test_plans_keep_timing_audit_and_satskip_separate(self):
        def plan(args):
            a = B.parser().parse_args(args)
            return B.commands(a, Path('/repo'), Path('/worker'), Path('/new-out'))
        stable = plan(['run'])
        self.assertTrue(stable[0][0].endswith('/raytrace_stable_bench'))
        self.assertNotIn('--original', stable[0])
        self.assertTrue(stable[1][1].endswith('/extract.py'))
        self.assertIn('--original', plan(['run', '--policy', 'original'])[0])
        self.assertEqual(plan(['audit'])[0][-1], 'audit')
        self.assertEqual(plan(['audit', '--three-lane-check'])[0][-1], 'check')
        self.assertEqual(plan(['sat-skip', '--checked'])[0][-2:], ['32', 'check'])
        self.assertEqual(plan(['sat-skip'])[0][-2:], ['512', 'time'])

    def test_failed_graph_can_extract_but_keeps_failed_exit(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); source = root/'input.egg'; source.write_text('; input')
            worker = root/'worker'; worker.write_text('fixture')
            out = root/'run'; calls = []
            def fake_run(cmd, **kwargs):
                calls.append(cmd)
                if cmd[0].endswith('raytrace_stable_bench'):
                    out.mkdir(); r = fixture(); r['same_commit_verification'] = None
                    (out/'report.json').write_text(json.dumps(r))
                    return subprocess.CompletedProcess(cmd, 1)
                (out/'extraction').mkdir()
                (out/'extraction/quality.json').write_text('{"passed": true}')
                return subprocess.CompletedProcess(cmd, 0)
            with patch.object(B.subprocess, 'run', side_effect=fake_run), patch.object(B.subprocess, 'check_output', return_value='test\n'), contextlib.redirect_stdout(io.StringIO()):
                status = B.main(['--repo',str(root),'run','--snapshot',str(source),'--worker',str(worker),'--no-build','--output',str(out)])
            self.assertEqual(status, 1)
            self.assertEqual(len(calls), 2)
            self.assertEqual(json.loads((out/'workflow.json').read_text())['returncode'], 1)

    def test_dry_run_has_no_writes_or_subprocesses(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)/'never-created'
            with patch.object(B.subprocess,'run',side_effect=AssertionError('executed')), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(B.main(['--dry-run','run','--output',str(out)]),0)
            self.assertFalse(out.exists())

    def test_saved_cpu_diagnostic_failure_is_not_a_successful_validation(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder=Path(tmp)
            (folder/'cpu-pair.json').write_text('{"exact_graph":null,"graph_error":"different facts"}')
            self.assertFalse(B.saved_gate('cpu-pair',folder))
            (folder/'cpu-pair.json').write_text('{"exact_graph":{"exact_rows_values_equivalence_visibility":true}}')
            self.assertTrue(B.saved_gate('cpu-pair',folder))

    def test_saturation_mode_does_not_require_coverage_or_mutate_saved_reports(self):
        old = REPO/'skills/eggcc-pass-benchmark/scripts/bench.py'
        with tempfile.TemporaryDirectory() as tmp:
            result = subprocess.run([sys.executable,str(old),'--repo',tmp,'--dry-run','saturate-run','--output','out/new'],capture_output=True,text=True,check=True)
            self.assertIn('--timing-only',result.stdout)
            self.assertNotIn('--library',result.stdout)
            self.assertNotIn('ripsat_coverage',result.stdout)
            self.assertFalse((Path(tmp)/'out').exists())
        folder = REPO/'experiments/eggcc_passes/raytrace_saturation/results'
        before = {p:hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.rglob('*') if p.is_file()}
        summarize = module(REPO/'experiments/eggcc_passes/saturation_report.py','sat_report').summarize
        with contextlib.redirect_stdout(io.StringIO()): result = summarize(folder,write=False)
        self.assertEqual(before,{p:hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.rglob('*') if p.is_file()})
        totals = result['workloads'][0]['total']
        self.assertAlmostEqual(totals['saturate_share_of_schedule'], totals['saturate_seconds']/(totals['saturate_seconds']+totals['run_seconds']))

if __name__ == '__main__':
    unittest.main()
