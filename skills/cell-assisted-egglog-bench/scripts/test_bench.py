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
    def make_cache(self, root):
        source = root/'input.egg'; source.write_text('; same input')
        worker = root/'worker'; worker.write_text('worker')
        binary = root/'target/analyzer/release/examples/raytrace_stable_bench'
        binary.parent.mkdir(parents=True); binary.write_text('binary')
        args = B.parser().parse_args(['run', '--snapshot', str(source),
                                     '--worker', str(worker), '--no-extract'])
        env = dict(RAYON_NUM_THREADS='1', EGGLOG_PROBE_COUNTS_OFF='1',
                   CELL_MATRIX_WORKER=str(worker))
        folder = root/'out/old'; folder.mkdir(parents=True)
        workflow = dict(mode='run', returncode=0, snapshot_sha256=B.sha(source), policy='stable',
                        inference_identity=B.inference_identity(args, root, worker, env))
        (folder/'workflow.json').write_text(json.dumps(workflow))
        (folder/'report.json').write_text(json.dumps(fixture()))
        return args, worker, env, folder

    def test_cache_requires_identical_execution_and_successful_graph_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a, worker, env, folder = self.make_cache(root)
            self.assertEqual(B.reusable_result(a, root, worker, env), folder)
            for file in [a.snapshot, worker, root/'target/analyzer/release/examples/raytrace_stable_bench']:
                old = file.read_text(); file.write_text('changed')
                self.assertIsNone(B.reusable_result(a, root, worker, env))
                file.write_text(old)
            self.assertIsNone(B.reusable_result(a, root, worker, dict(env, RAYON_NUM_THREADS='4')))
            a.joint_sat = True
            self.assertIsNone(B.reusable_result(a, root, worker, env))
            a.joint_sat = False
            r = fixture(); r['same_commit_verification'] = None
            (folder/'report.json').write_text(json.dumps(r))
            self.assertIsNone(B.reusable_result(a, root, worker, env))

    def test_old_provenance_is_history_not_current_binary_certification(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a, worker, env, folder = self.make_cache(root)
            w = B.load(folder/'workflow.json'); w.pop('inference_identity')
            (folder/'workflow.json').write_text(json.dumps(w))
            self.assertEqual(len(list(B.matching_history(root, a.snapshot, 'stable', False))), 1)
            self.assertIsNone(B.reusable_result(a, root, worker, env))
            with patch.object(B.subprocess, 'run', side_effect=AssertionError('executed')), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(B.main(['--repo', str(root), 'history', '--snapshot', str(a.snapshot)]), 0)

    def test_cache_hit_never_executes_inference_or_creates_an_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a, worker, env, folder = self.make_cache(root)
            out = root/'out/new'
            with patch.dict(B.os.environ, env, clear=True), \
                 patch.object(B.subprocess, 'run', side_effect=AssertionError('executed')), \
                 contextlib.redirect_stdout(io.StringIO()) as text:
                status = B.main(['--repo', str(root), 'run', '--no-build', '--no-extract',
                                 '--snapshot', str(a.snapshot), '--worker', str(worker), '--output', str(out)])
            self.assertEqual(status, 0)
            self.assertIn('没有重新执行推理', text.getvalue())
            self.assertFalse(out.exists())

    def test_requested_extraction_is_not_satisfied_by_inference_only_cache(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a, worker, env, _ = self.make_cache(root)
            a.no_extract = False
            self.assertIsNone(B.reusable_result(a, root, worker, env))

    def test_refresh_bypasses_cached_result(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a, worker, env, _ = self.make_cache(root)
            with patch.dict(B.os.environ, env, clear=True), \
                 patch.object(B, 'reusable_result', side_effect=AssertionError('looked up cache')), \
                 patch.object(B.subprocess, 'run', return_value=subprocess.CompletedProcess([], 7)) as execute, \
                 contextlib.redirect_stdout(io.StringIO()):
                status = B.main(['--repo', str(root), 'run', '--no-build', '--no-extract', '--refresh',
                                 '--snapshot', str(a.snapshot), '--worker', str(worker),
                                 '--output', str(root/'out/new')])
            self.assertEqual(status, 7)
            execute.assert_called_once()

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
        self.assertIn('--joint-sat', plan(['run', '--joint-sat'])[0])
        self.assertEqual(plan(['audit'])[0][-1], 'audit')
        self.assertEqual(plan(['audit', '--three-lane-check'])[0][-1], 'check')
        self.assertEqual(plan(['sat-skip', '--checked'])[0][-2:], ['32', 'check'])
        self.assertEqual(plan(['sat-skip'])[0][-2:], ['512', 'time'])
        self.assertEqual(plan(['parameterized'])[0], ['/repo/target/analyzer/release/examples/parameterized_cell', '/new-out'])
        self.assertEqual(plan(['transition'])[0], ['/repo/target/analyzer/release/examples/cell_transition', '/new-out'])
        self.assertEqual(plan(['joint-sat'])[0], ['/repo/target/analyzer/release/examples/joint_sat_bench', '/worker', '/new-out'])
        self.assertEqual(plan(['joint-sat','--snapshot','/input.egg'])[0][-1],'/input.egg')

    def test_joint_gate_requires_every_lane_and_boundary(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder=Path(tmp)
            proof={'exact_rows_values_equivalence_visibility':True}
            r={'joint_sat_verified':True,'lanes':[{'phases':[{'verification':proof}]} for _ in range(2)]}
            (folder/'results.json').write_text(json.dumps(r))
            self.assertTrue(B.saved_gate('joint-sat',folder))
            r['lanes'][1]['phases'][0]['verification']=None
            (folder/'results.json').write_text(json.dumps(r))
            self.assertFalse(B.saved_gate('joint-sat',folder))

    def test_joint_speedup_requires_its_gpu_control_graph_check(self):
        r=fixture();r['gpu_without_joint']=dict(audit=False,runtime=dict(run_seconds=5))
        self.assertIsNone(B.metrics(r)['joint_speedup'])
        self.assertIsNone(B.metrics(r)['inference_speedup'])
        r['joint_verification']=dict(exact_rows_values_equivalence_visibility=True)
        self.assertEqual(B.metrics(r)['joint_speedup'],1.25)

    def test_parameterized_gate_requires_both_graphs_and_counts(self):
        phase = dict(same_submit=dict(exact_rows_values_equivalence_visibility=True),
                     fused_native=dict(exact_rows_values_equivalence_visibility=True),
                     same_per_rule_binding_counts=True, mirror_rows_checked=9)
        r = dict(passed=True, phases=[phase])
        self.assertTrue(B.parameterized_gate(r))
        for key in phase:
            changed=copy.deepcopy(r); changed['phases'][0].pop(key)
            self.assertFalse(B.parameterized_gate(changed))
        self.assertFalse(B.parameterized_gate(dict(passed=True, phases=[])))

    def test_parameterized_dry_run_does_not_build_or_require_gpu(self):
        with patch.object(B.subprocess,'run',side_effect=AssertionError('executed')), contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(B.main(['--dry-run','parameterized']),0)
        text=output.getvalue()
        self.assertIn('--example parameterized_cell',text)
        self.assertNotIn('swiftc',text)

    def test_transition_gate_does_not_accept_a_failed_ablation(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            proof = dict(exact_rows_values_equivalence_visibility=True)
            r = dict(passed=True, port_transition_verified=True,
                     phases=[dict(same_submit=proof, reuse_off_on=proof)])
            (folder/'results.json').write_text(json.dumps(r))
            self.assertTrue(B.saved_gate('transition', folder))
            r['phases'][0]['reuse_off_on'] = None
            (folder/'results.json').write_text(json.dumps(r))
            self.assertFalse(B.saved_gate('transition', folder))
        with patch.object(B.subprocess, 'run', side_effect=AssertionError('executed')), contextlib.redirect_stdout(io.StringIO()) as text:
            self.assertEqual(B.main(['--dry-run', 'transition']), 0)
        self.assertNotIn('swiftc', text.getvalue())

    def test_loaded_transfer_report_does_not_claim_independent_graph_or_timing_proof(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'results.json'
            path.write_text(json.dumps(dict(transfer_run_complete=True,passed=True,
                boundaries=[{}],runtime=dict(stats=dict(loaded_programs=7,program_compilations=0)))))
            with contextlib.redirect_stdout(io.StringIO()) as text:
                B.report(path)
            self.assertIn('不是独立原生整图对照', text.getvalue())
            self.assertNotIn('差异定位', text.getvalue())

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
