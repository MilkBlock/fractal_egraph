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

    def interaction_fixture(self, root):
        paths={}
        for name,sat,refute,visits in [('s0r0',False,False,1000),('s0r1',False,True,400),('s1r0',True,False,900),('s1r1',True,True,300)]:
            path=root/name;path.mkdir();paths[name]=path
            rules=[dict(ruleset='s',rule='removed',stats=dict(eligible_seeds=0 if sat else 100,refuted_seeds=0,productive_seeds=0)),
                   dict(ruleset='s',rule='common',stats=dict(eligible_seeds=10,refuted_seeds=5 if refute else 0,productive_seeds=2))]
            work={f:sum(q['stats'][f] for q in rules) for f in ['eligible_seeds','refuted_seeds','productive_seeds']}
            work.update(join_live_row_visits=visits,join_probes=visits)
            r=fixture();r.update(schedule=['s'],assisted_rulesets=['s'])
            r['gpu'].update(cells=dict(profile=True,refutation=refute,refutation_signatures=refute,refutation_domains=False),
                            saturation_diagnostics={'joint':{}} if sat else None,cell_query_reuse=True,cell_work=work,rules=rules)
            r['gpu']['runtime'].update(cell_query_bindings=0,cell_query_rules=1 if sat else 0)
            (path/'report.json').write_text(json.dumps(r))
            (path/'workflow.json').write_text(json.dumps(dict(snapshot_sha256='input',binary_sha256='binary',worker_sha256='worker',policy='stable')))
        return paths

    def test_interaction_separates_rule_mix_from_stronger_refutation(self):
        with tempfile.TemporaryDirectory() as tmp:
            result=B.interaction_report(self.interaction_fixture(Path(tmp)))
            self.assertAlmostEqual(result['records']['s0r1']['rejection_rate'],5/110)
            self.assertEqual(result['records']['s1r1']['rejection_rate'],.5)
            mix=result['query_mix'][0]
            self.assertEqual(mix['common_reference_rate'],.5)
            self.assertEqual(mix['common_candidate_fixed_weight_rate'],.5)
            self.assertEqual(mix['removed_rules'],1)
            self.assertEqual(result['contrasts']['join_live_row_visits']['superadditive_saving'],0)

    def test_interaction_rejects_mixed_binaries_and_inconsistent_counters(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths=self.interaction_fixture(Path(tmp));p=paths['s1r1']/'workflow.json';w=B.load(p)
            p.write_text(json.dumps(dict(w,binary_sha256='other')))
            with self.assertRaises(ValueError):B.interaction_report(paths)
            p.write_text(json.dumps(w))
            p=paths['s1r1']/'report.json';r=B.load(p);r['gpu']['cell_work']['eligible_seeds']+=1;p.write_text(json.dumps(r))
            with self.assertRaises(ValueError):B.interaction_report(paths)

    def test_interaction_no_residual_candidates_has_no_fake_hit_rate(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths=self.interaction_fixture(Path(tmp))
            for name in ['s1r0','s1r1']:
                p=paths[name]/'report.json';r=B.load(p)
                for row in r['gpu']['rules']:
                    row['stats']={k:0 for k in row['stats']}
                r['gpu']['cell_work']={k:0 for k in r['gpu']['cell_work']};p.write_text(json.dumps(r))
            result=B.interaction_report(paths)
            self.assertIsNone(result['records']['s1r1']['rejection_rate'])
            self.assertIsNone(result['query_mix'][0]['common_candidate_fixed_weight_rate'])

    def depth_fixture(self, root):
        paths=self.interaction_fixture(root)
        fields=['entries','link_visits','live_rows','prefixes','key_checks','refutations',
                'matched_leaves','domain_checks','signature_refutations','deepest_seeds']
        for name,path in paths.items():
            r=B.load(path/'report.json');g=r['gpu'];refute=name.endswith('r1')
            for q in g['rules']:
                q['atoms']=3
                if q['rule']=='removed':
                    e=q['stats']['eligible_seeds']
                    rows=[dict(entries=e,prefixes=e),dict(entries=e,link_visits=e,live_rows=e,deepest_seeds=e),{}]
                elif refute:
                    rows=[dict(entries=10,prefixes=10,key_checks=10,refutations=5,deepest_seeds=5),
                          dict(entries=5,link_visits=20,live_rows=20,prefixes=8,deepest_seeds=3),
                          dict(entries=8,link_visits=3,live_rows=3,prefixes=2,matched_leaves=2,deepest_seeds=2)]
                else:
                    rows=[dict(entries=10,prefixes=10),
                          dict(entries=10,link_visits=30,live_rows=30,prefixes=20,deepest_seeds=5),
                          dict(entries=20,link_visits=10,live_rows=10,prefixes=2,matched_leaves=2,deepest_seeds=5)]
                q['stats']['depth_work']=[[row.get(f,0) for f in fields] for row in rows]
            g['join_depth_fields']=fields
            total={f:sum(row[i] for q in g['rules'] for row in q['stats']['depth_work']) for i,f in enumerate(fields)}
            w=g['cell_work'];w.update(join_probes=sum(row[0] for q in g['rules'] for row in q['stats']['depth_work'][1:]),
                join_link_visits=total['link_visits'],join_live_row_visits=total['live_rows'],refutation_checks=total['key_checks'],
                domain_checks=0,signature_refuted_branches=0,refuted_branches=total['refutations'],domain_refuted_branches=0)
            (path/'report.json').write_text(json.dumps(r))
        return paths

    def test_depth_branch_entries_are_not_seed_reach_probabilities(self):
        with tempfile.TemporaryDirectory() as tmp:
            result=B.join_depth_report(self.depth_fixture(Path(tmp)))
            d=result['depths']['s1r0'][2]
            self.assertEqual(d['entries_per_seed'],2)
            self.assertEqual(d['reach_probability'],.5)
            for rows in result['depths'].values():
                self.assertAlmostEqual(sum(r['deepest_seed_probability'] for r in rows),1)
                self.assertTrue(all(0<=r['reach_probability']<=1 for r in rows))

    def test_depth_checks_prefix_flow_and_one_deepest_bin_per_seed(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths=self.depth_fixture(Path(tmp));p=paths['s1r1']/'report.json';original=B.load(p)
            for field in ['prefixes','deepest_seeds']:
                r=copy.deepcopy(original);i=r['gpu']['join_depth_fields'].index(field)
                r['gpu']['rules'][1]['stats']['depth_work'][1][i]+=1;p.write_text(json.dumps(r))
                with self.assertRaises(ValueError):B.join_depth_report(paths)
            p.write_text(json.dumps(original));del original['gpu']['join_depth_fields'];p.write_text(json.dumps(original))
            with self.assertRaises(ValueError):B.join_depth_report(paths)

    def test_depth_preserves_legacy_probe_counter_retry_semantics(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths=self.depth_fixture(Path(tmp));p=paths['s1r1']/'report.json';r=B.load(p)
            r['gpu']['cell_work']['join_probes']-=2;p.write_text(json.dumps(r))
            result=B.join_depth_report(paths)['records']['s1r1']
            self.assertEqual(result['compact_probe_attempts']-result['legacy_final_attempt_probes'],2)

    def test_depth_csv_exports_saved_data_without_running_inference(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);paths=self.depth_fixture(root);out=root/'depth.csv'
            args=['--repo',str(root),'depth']
            for name,path in paths.items():args += ['--'+name,str(path)]
            with patch.object(B.subprocess,'run',side_effect=AssertionError('inference executed')),contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(B.main(args+['--csv',str(out)]),0)
            with out.open() as f:rows=list(B.csv.DictReader(f))
            self.assertEqual(len(rows),12)
            self.assertIn('deepest_seeds',rows[0]);self.assertIn('reach_probability',rows[0])

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

    def test_explicit_control_cache_fingerprints_the_saved_proof(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            a,worker,env,folder=self.make_cache(root)
            a.reuse_controls=folder
            (folder/'graphs').mkdir()
            for lane in ['native','cpu']:
                (folder/'graphs'/f'{lane}.control.json').write_text('{}')
            before=B.inference_identity(a,root,worker,env)
            self.assertIsNotNone(before)
            (folder/'graphs/cpu.control.json').write_text('{"changed":true}')
            self.assertNotEqual(before,B.inference_identity(a,root,worker,env))

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

    def test_phase_profile_is_not_reported_as_a_timing_speedup(self):
        r=fixture();r['gpu']['query_timing_profile']=True
        m=B.metrics(r)
        self.assertTrue(m['exact_cpu_gpu']);self.assertFalse(m['timing_eligible'])
        self.assertIsNone(m['inference_speedup'])

    def test_search_atomics_are_not_reported_as_uninstrumented_timing(self):
        r = fixture(); r['gpu']['cells'] = {'profile': True}
        self.assertFalse(B.metrics(r)['timing_eligible'])
        self.assertIsNone(B.metrics(r)['inference_speedup'])

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
        self.assertEqual(plan(['audit', '--all-rules', '--joint-sat'])[0][-3:], ['--all-rules', '--audit-only', '--joint-sat'])
        self.assertEqual(plan(['run', '--all-rules', '--joint-sat'])[0][-2:], ['--joint-sat', '--all-rules'])
        self.assertEqual(plan(['audit','--all-rules','--cell-only','--joint-sat'])[0][-3:], ['--all-rules','--cell-audit-only','--joint-sat'])
        self.assertEqual(plan(['audit'])[0][-1], 'audit')
        self.assertEqual(plan(['audit', '--three-lane-check'])[0][-1], 'check')
        self.assertEqual(plan(['sat-skip', '--checked'])[0][-2:], ['32', 'check'])
        self.assertEqual(plan(['sat-skip'])[0][-2:], ['512', 'time'])
        self.assertEqual(plan(['parameterized'])[0], ['/repo/target/analyzer/release/examples/parameterized_cell', '/new-out'])
        self.assertEqual(plan(['transition'])[0], ['/repo/target/analyzer/release/examples/cell_transition', '/new-out'])
        self.assertEqual(plan(['transfer-bench'])[0], ['/repo/target/analyzer/release/examples/transfer_bench',
            '/repo/experiments/parameterized_cell/results/transfers.egg', '/new-out', '512'])
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

    def test_transfer_timing_requires_all_three_graph_checks_and_no_gpu_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder=Path(tmp)
            proof=dict(exact_rows_values_equivalence_visibility=True)
            r=dict(transfer_bench_verified=True,verification=[dict(native=proof,loaded=proof,joint=proof)])
            (folder/'results.json').write_text(json.dumps(r))
            self.assertTrue(B.saved_gate('transfer-bench',folder))
            r['verification'][0]['native']=None
            (folder/'results.json').write_text(json.dumps(r))
            self.assertFalse(B.saved_gate('transfer-bench',folder))
        with patch.object(B.subprocess,'run',side_effect=AssertionError('executed')), contextlib.redirect_stdout(io.StringIO()) as text:
            self.assertEqual(B.main(['--dry-run','transfer-bench']),0)
        self.assertNotIn('swiftc',text.getvalue())

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
