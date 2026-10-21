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

    def test_analyze_credit_keeps_public_executor_and_rejects_changed_library(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);paths={n:root/n for n in ['baseline','plan_only','candidate']}
            for name,p in paths.items():
                p.mkdir();r=fixture();r.update(schedule=['S'],assisted_rulesets=['s'])
                g=r['gpu'];g.update(device={'device_bytes':100},cells={
                    'profile':False,'plan_only':name=='plan_only',
                    'library':None if name=='baseline' else {'loaded_states':[0]},
                    'plans':[],'projection_queries':[],'projection_fallback_queries':[]})
                g['runtime']['run_seconds']={'baseline':5,'plan_only':4.5,'candidate':4}[name]
                w=dict(snapshot_sha256='s',binary_sha256='b',worker_sha256='w',policy='stable',
                       inference_identity={'analysis':None if name=='baseline' else {'state':'hash'}})
                (p/'report.json').write_text(json.dumps(r));(p/'workflow.json').write_text(json.dumps(w))
            result=B.analyze_credit_report(paths)
            self.assertEqual(result['net_library_seconds'],1)
            p=paths['candidate']/'workflow.json';w=B.load(p);w['inference_identity']['analysis']={'state':'different'};p.write_text(json.dumps(w))
            with self.assertRaisesRegex(ValueError,'libraries differ'):B.analyze_credit_report(paths)

    def test_analysis_library_contents_are_part_of_cache_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); a,worker,env,*_=self.make_cache(root)
            lib=root/'library'; (lib/'catalog/states').mkdir(parents=True)
            (lib/'catalog/catalog.json').write_text('{}')
            state=lib/'catalog/states/state-0000.json'; state.write_text('{}')
            env['CELL_BENCH_ANALYSIS']=str(lib)
            before=B.inference_identity(a,root,worker,env)
            state.write_text('{"changed":true}')
            self.assertNotEqual(before,B.inference_identity(a,root,worker,env))


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

    def test_connection_depth_pair_keeps_routing_fixed_and_rejects_invalid_proofs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);fixtures=self.depth_fixture(root)
            paths={'before':root/'before','after':root/'after'}
            base=B.load(fixtures['s1r1']/'report.json')
            for name,path in paths.items():
                path.mkdir();r=copy.deepcopy(base);r['gpu']['cells']['binding_ranges']=name=='after'
                fields=['connection_checks','connection_refutations','connection_unknown','range_checks','range_refutations']
                r['gpu']['join_depth_fields']+=fields
                for q in r['gpu']['rules']:
                    for row in q['stats']['depth_work']:row.extend([0]*len(fields))
                r['gpu']['cell_work'].update({f:0 for f in fields})
                (path/'report.json').write_text(json.dumps(r))
                (path/'workflow.json').write_text((fixtures['s1r1']/'workflow.json').read_text())
            result=B.join_depth_report(paths,True)
            self.assertTrue(result['depth_contrasts']['connection_effect']['same_per_rule_seed_counts'])
            p=paths['after']/'report.json';r=B.load(p);r['gpu']['runtime']['cell_query_bindings']+=1;p.write_text(json.dumps(r))
            with self.assertRaises(ValueError):B.join_depth_report(paths,True)
            r['gpu']['runtime']['cell_query_bindings']-=1;r['gpu']['cell_work']['range_refutations']+=1;p.write_text(json.dumps(r))
            with self.assertRaises(ValueError):B.join_depth_report(paths,True)

    def test_cell_cases_distinguishes_empty_answers_cache_hits_and_service(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);paths=self.depth_fixture(root);current=root/'current';current.mkdir()
            r=B.load(paths['s1r1']/'report.json');g=r['gpu']
            for q in g['rules']:
                q['stats'].update(emitted_bindings=0,cell_queries=0,cell_bindings=0,dispatches=0,quiet_runs=0,shared_queries=0,cell_nonempty_queries=0)
            for name in ['s0r1','s1r1']:
                old=B.load(paths[name]/'report.json')
                for q in old['gpu']['rules']:
                    q['stats'].update(emitted_bindings=0,cell_queries=0,cell_bindings=0)
                (paths[name]/'report.json').write_text(json.dumps(old))
            ps=[]
            for i,q in enumerate(g['rules']):
                ps.append(dict(program_id=i,kind='scalar_query',ruleset=q['ruleset'],rule=q['rule'],seed='Empty' if i==0 else 'Pair',
                     index_reads=3*i,reused_port_reads=i,seed_evaluations=2*i,resumed_seed_evaluations=i,emitted_bindings=0,live_seed_instances=i,stored_port_paths=i))
            g['saturation_diagnostics'].update(program_telemetry_version=2,programs=ps,
                port_reuse=dict(index_reads=3,reused_reads=1,resumed_seed_evaluations=1,stored_paths=1),joint=dict(stats=dict(macro_instances=0),comparison_closure={}))
            # Empty, complete query responses are handled, but have no cache
            # lookups or positive answers. Historical after has the same work.
            g['rules'][0]['stats']['cell_queries']=4
            old=B.load(paths['s1r1']/'report.json');old['gpu']['rules'][0]['stats']['cell_queries']=4
            (paths['s1r1']/'report.json').write_text(json.dumps(old))
            (current/'report.json').write_text(json.dumps(r));(current/'workflow.json').write_text((paths['s1r1']/'workflow.json').read_text())
            out=B.cell_cases_report(current,paths['s0r1'],paths['s1r1']);a,b=out['programs']
            self.assertEqual(a['query_service_rate'],1);self.assertEqual(a['nonempty_answer_rate'],0)
            self.assertIsNone(a['port_read_hit_rate']);self.assertAlmostEqual(b['port_read_hit_rate'],.25)
            g['saturation_diagnostics']['programs'].pop();(current/'report.json').write_text(json.dumps(r))
            with self.assertRaises(ValueError):B.cell_cases_report(current,paths['s0r1'],paths['s1r1'])

    def test_cell_value_counts_empty_and_zero_cache_cases_without_free_work(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);paths=self.depth_fixture(root);current=root/'current';current.mkdir()
            report=B.load(paths['s1r1']/'report.json')
            report['gpu']['saturation_diagnostics'].update(stats={'committed_row_events':12,'union_events':2},dependency_edges=5)
            (current/'report.json').write_text(json.dumps(report))
            def program(i,starts,visits,roots,reads):
                return dict(program_id=i,kind='scalar_query',ruleset='s',root='Empty' if i==0 else 'Get',rule=str(i),
                    gpu_seeds_before=starts,gpu_seeds_after=0,gpu_rows_saved=visits,seed_evaluations=roots,
                    fresh_reads=reads,cached_reads=0,cell_query_requests=4,retired_guard_seeds=0)
            cases=dict(scope='test',provenance={},programs=[program(0,100,0,0,0),program(1,5,10,4,3),program(2,0,-5,2,2)],
                comparison_closure={'seed_evaluations':7,'cost_lookups':3},port_reuse=dict(index_reads=5,reused_reads=0,stored_paths=2))
            with patch.object(B,'cell_cases_report',return_value=cases):
                r=B.cell_value_report(current,paths['s0r1'],paths['s1r1'])
            empty,get,bad=r['programs']
            self.assertGreater(empty['observed_start_delta_share'],0);self.assertEqual(empty['candidate_rows_removed'],0)
            self.assertEqual(empty['cell_query_requests'],4)
            self.assertGreater(get['observed_candidate_delta_share'],0);self.assertEqual(get['cell_new_port_reads'],3)
            self.assertLess(bad['observed_candidate_delta_share'],0)
            self.assertIsNone(r['net_inference_benefit']);self.assertIsNone(r['per_cell_marginal_time'])
            self.assertEqual(r['local_work']['shared_comparison_proof']['cost_lookups'],3)
            self.assertEqual(r['totals']['candidate_rows']['saved'],r['unassigned']['candidate_rows']+sum(p['candidate_rows_removed'] for p in r['programs']))

    def test_cell_net_benefit_preserves_slowdowns_and_rejects_confounded_time(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);paths=[root/'before',root/'after'];rs=[]
            w=dict(snapshot_sha256='input',binary_sha256='bin',worker_sha256='worker',policy='stable')
            for i,path in enumerate(paths):
                path.mkdir();r=fixture();r.update(schedule=['s'],assisted_rulesets=['s'])
                r['gpu'].update(cells=dict(refutation=True),saturation_diagnostics={} if i==0 else {'joint':{}})
                r['gpu']['runtime']['run_seconds']=10+2*i;rs.append(r)
                (path/'report.json').write_text(json.dumps(r));(path/'workflow.json').write_text(json.dumps(w))
            result=B.cell_net_time(*paths);self.assertEqual(result['saved_seconds'],-2);self.assertEqual(result['fraction_saved'],-.2)
            for field in ['profile','refutation']:
                r=copy.deepcopy(rs[1]);r['gpu']['cells'][field]=True if field=='profile' else False
                (paths[1]/'report.json').write_text(json.dumps(r))
                with self.assertRaises(ValueError):B.cell_net_time(*paths)
            (paths[1]/'report.json').write_text(json.dumps(rs[1]))
            (paths[1]/'workflow.json').write_text(json.dumps(dict(w,binary_sha256='other')))
            with self.assertRaises(ValueError):B.cell_net_time(*paths)

    def test_credit_keeps_generic_executor_and_requires_consumed_proofs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);paths=[root/'base',root/'cell'];rs=[]
            w=dict(snapshot_sha256='s',binary_sha256='b',worker_sha256='w',policy='stable')
            for i,path in enumerate(paths):
                path.mkdir();r=fixture();r.update(schedule=['s'],assisted_rulesets=['s'])
                # Negative credit must survive; consuming proof isn't itself a win.
                r['gpu']['runtime'].update(run_seconds=10+i,setup_seconds=1)
                program=dict(ruleset='s',rule='get',kind='scalar_query',seed='Get',accesses=[],retired_guard_seeds=0,seed_evaluations=3,index_reads=3,reused_port_reads=0)
                r['gpu'].update(cells={},cell_query_reuse=True,rules=[dict(ruleset='s',rule='get',stats=dict(cell_queries=2,cell_bindings=3))],
                    saturation_diagnostics=dict(port_reuse=dict(enabled=True),programs=[program],
                    joint=dict(credit_contract='same-dataflow-optional-joint-proofs/v1',proofs_enabled=bool(i),
                    stats=dict(macro_instances=0,skipped_joint_saturations=0),comparison_closure=dict(saturations=i))))
                (path/'report.json').write_text(json.dumps(r));(path/'workflow.json').write_text(json.dumps(w));rs.append(r)
            r=B.cell_credit_report(*paths);self.assertEqual(r['credit_seconds'],-1)
            self.assertEqual(r['common_query_paths'][0]['ordinary_handoff_credit'],0)
            self.assertEqual(r['common_query_paths'][0]['fresh_reads'],[3,3])
            self.assertIsNone(r['individual_cell_credits'])
            bad=copy.deepcopy(rs[0]);bad['gpu']['cell_query_reuse']=False;(paths[0]/'report.json').write_text(json.dumps(bad))
            with self.assertRaises(ValueError):B.cell_credit_report(*paths)
            (paths[0]/'report.json').write_text(json.dumps(rs[0]))
            bad=copy.deepcopy(rs[1]);bad['gpu']['saturation_diagnostics']['joint']['comparison_closure']['saturations']=0
            (paths[1]/'report.json').write_text(json.dumps(bad))
            with self.assertRaises(ValueError):B.cell_credit_report(*paths)

    def test_empty_table_report_counts_generic_pruning_without_cell_credit(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=self.depth_fixture(root)
            paths={'before':source['s0r1'],'after':source['s1r1']}
            for name,path in paths.items():
                r=B.load(path/'report.json');g=r['gpu'];g['saturation_diagnostics']=None
                g['gpu_optimizations']={'empty_table_pruning':name=='after'}
                g['runtime'].update(cell_query_rules=0,cell_query_bindings=0,gpu_empty_table_prunes=int(name=='after'))
                for i,q in enumerate(g['rules']):
                    q['stats'].update(empty_table_prunes=int(name=='after' and i==0),emitted_bindings=2 if i else 0)
                (path/'report.json').write_text(json.dumps(r))
            out=B.join_depth_report(paths,empty_tables=True)
            self.assertEqual(out['records']['after']['empty_table_query_requests'],1)
            self.assertEqual(out['pruned_rules'][0]['seeds_after'],0)
            self.assertFalse(out['depth_contrasts']['empty_table_effect']['same_per_rule_seed_counts'])
            p=paths['after']/'report.json';r=B.load(p);r['gpu']['runtime']['cell_query_rules']=1;p.write_text(json.dumps(r))
            with self.assertRaises(ValueError):B.empty_table_pair_report(paths)

    def test_cell_credit_rejects_mixed_generic_gpu_optimizations(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);paths=self.interaction_fixture(root)
            p=paths['s1r1']/'report.json';r=B.load(p);r['gpu']['gpu_optimizations']={'empty_table_pruning':True};p.write_text(json.dumps(r))
            with self.assertRaises(ValueError):B.interaction_report(paths)

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
        r['reused_controls']={'directory':'old-other-binary'}
        self.assertIsNone(B.metrics(r)['joint_speedup'], 'historical GPU-off is not a proof-only control')

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
