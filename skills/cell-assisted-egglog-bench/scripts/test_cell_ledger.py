import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import bench as B
import cell_ledger as L

FIELDS=['attempts','probes','missing_keys','ready_publications','segment_checks','segment_hits','skipped_internal_reads','port_queries','port_nonempty_queries','port_matches','port_probes','port_candidate_rows','watch_checks','watch_wakes','invalidations','row_reuses']

def fixture(profile=True,only=False):
    qs=[{'rule':'R'+str(i),'ruleset':'s','stats':{'dispatches':5,'emitted_bindings':2,'empty_table_prunes':0,'cell_queries':0,'cell_bindings':0,'search_work':[10,20,80 if only else 50]}} for i in range(2)]
    return {'benchmark_finished':True,'schedule':['s'],'assisted_rulesets':['s'],
        'same_commit_verification':{'exact_rows_values_equivalence_visibility':True},
        'gpu':{'audit':False,'runtime':{'run_seconds':4},'device':{'device_bytes':300},
            'work_profile_version':1,'search_work_fields':['starts','index_probes','candidate_rows'],
            'cell_plan_fields':FIELDS,'cell_plan_work':{'0':[5,4,2,3,8,3,3,4,2,2,6,7,10,1,0,2]},
            'cell_work':{'watch_entries_checked':14},'gpu_optimizations':{'empty_table_pruning':True},
            'cells':{'profile':profile,'plan_only':only,'library':{'loaded_states':[0,1,2,3],'rejected':[{'state':4,'reason':'unsupported'}]},
                'plans':[{'plan':0,'states':[0,1],'atoms':[0,1],'consumers':[0,1]}, {'plan':1,'states':[2],'atoms':[1,2],'consumers':[1]}]},
            'rules':qs,'saturation_diagnostics':{'programs':[{'program_id':0,'seed':'Get','ruleset':'s','rule':'R0','index_reads':9,'reused_port_reads':3}]}}}

def save(root,name,r):
    p=root/name;p.mkdir();(p/'report.json').write_text(json.dumps(r))
    (p/'workflow.json').write_text(json.dumps({'snapshot_sha256':'s','binary_sha256':'b','worker_sha256':'w','policy':'stable','inference_identity':{'analysis':{'a':'b'}}}))
    return p

class LedgerTests(unittest.TestCase):
    def test_all_cells_and_shared_work_are_kept_without_duplicate_credit(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);p=save(root,'on',fixture());b=save(root,'off',fixture(only=True))
            r=L.build(p,b)
            self.assertEqual(len([x for x in r['rows'] if x['kind']=='analyzed_cell']),5)
            self.assertEqual(r['groups'][0]['candidates_saved'],60)
            self.assertEqual(r['groups'][0]['plans'],[0,1])
            self.assertEqual(r['groups'][0]['owners'],[0,1,2])
            self.assertTrue(all(x['net_seconds'] is None for x in r['rows']))
            self.assertTrue(all(x['candidates_saved'] is None for x in r['rows'] if x['kind']=='analyzed_cell'))
            plan=next(x for x in r['rows'] if x['kind']=='shared_projection' and x['id']=='0')
            self.assertEqual(plan['hit_rate'],3/8);self.assertEqual(plan['port_hit_rate'],1/2)
            self.assertEqual(r['shared_costs']['watch_checks_not_assigned_to_valid_plan'],4)

    def test_unprofiled_or_old_data_is_not_zero(self):
        for mode in ['unprofiled','old']:
            with tempfile.TemporaryDirectory() as tmp:
                r=fixture(profile=mode!='unprofiled')
                if mode=='old':r['gpu'].pop('work_profile_version')
                p=save(Path(tmp),'run',r);out=L.build(p)
                plan=next(x for x in out['rows'] if x['kind']=='shared_projection')
                self.assertIsNone(plan['hits']);self.assertIsNone(plan['new_reads'])
                program=next(x for x in out['rows'] if x['kind']=='source_program')
                self.assertEqual(program['cache_hit_rate'],.25)

    def test_negative_candidate_delta_is_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);on=fixture();on['gpu']['rules'][0]['stats']['search_work'][2]=100
            p=save(root,'on',on);b=save(root,'off',fixture(only=True))
            r=L.build(p,b);consumer=next(x for x in r['rows'] if x['kind']=='consumer' and x['id']=='0')
            self.assertEqual(consumer['candidates_saved'],-20)

    def test_controls_cannot_change_generic_options_outputs_or_library(self):
        for kind in ['options','outputs','library','binary']:
            with tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);p=save(root,'on',fixture());r=fixture(only=True)
                if kind=='options':r['gpu']['gpu_optimizations']['empty_table_pruning']=False
                if kind=='outputs':r['gpu']['rules'][0]['stats']['emitted_bindings']=0
                b=save(root,'off',r)
                if kind in ['library','binary']:
                    w=json.loads((b/'workflow.json').read_text())
                    if kind=='binary':w['binary_sha256']='changed'
                    else:w['inference_identity']['analysis']={'different':'hash'}
                    (b/'workflow.json').write_text(json.dumps(w))
                with self.assertRaises(ValueError):L.build(p,b)

    def test_files_are_deterministic_and_unknown_files_are_protected(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=save(Path(tmp),'run',fixture());L.write(p)
            saved=(p/'cell-ledger.md').read_bytes();L.write(p)
            self.assertEqual(saved,(p/'cell-ledger.md').read_bytes())
            self.assertTrue((p/'cell-ledger.csv').exists())
            (p/'cell-ledger.json').write_text('{}')
            with self.assertRaises(ValueError):L.write(p)

    def test_no_cell_run_still_exports_empty_table(self):
        with tempfile.TemporaryDirectory() as tmp:
            r=fixture();r['gpu']['cells']={};r['gpu']['saturation_diagnostics']={}
            out=L.write(save(Path(tmp),'run',r))
            self.assertEqual(out['rows'],[])

    def test_invalid_graph_cannot_receive_candidate_credit(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);r=fixture();r['same_commit_verification']=None
            p=save(root,'on',r);b=save(root,'off',fixture(only=True))
            self.assertFalse(L.build(p)['verified'])
            with self.assertRaises(ValueError):L.build(p,b)

    def test_automatic_companions_reuse_or_collect_only_missing_profiles(self):
        for cached in [True,False]:
            with tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);p=save(root,'run',fixture(profile=False))
                a=B.parser().parse_args(['run','--all-rules','--joint-sat','--analysis',str(root/'lib'),'--no-extract'])
                old=os.environ.get('CELL_SEARCH_COUNTERS')
                answers=[root/'profile-plan',root/'profile-reuse'] if cached else [None,None]
                with patch.object(B,'reusable_result',side_effect=answers),patch.object(B,'main',return_value=0) as execute,patch.object(L,'write') as write:
                    B.finish_cell_metrics(a,root,root/'worker',p,{})
                    self.assertEqual(execute.call_count,0 if cached else 2)
                    if not cached:
                        self.assertTrue(all('--no-cell-metrics' in c.args[0] for c in execute.call_args_list))
                    self.assertEqual(write.call_count,1)
                self.assertEqual(os.environ.get('CELL_SEARCH_COUNTERS'),old)

    def test_commit_marker_costs_are_visible_and_flag_survives_companions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);r=fixture(profile=False)
            r['gpu']['commit_markers']={'directory_resolutions':12,'emitted_hints':4}
            p=save(root,'run',r)
            self.assertEqual(L.build(p)['shared_costs']['commit_marker_cpu_work']['directory_resolutions'],12)
            a=B.parser().parse_args(['run','--all-rules','--analysis',str(root/'lib'),'--commit-markers'])
            with patch.object(B,'reusable_result',return_value=None),patch.object(B,'main',return_value=0) as execute,patch.object(L,'write'):
                B.finish_cell_metrics(a,root,root/'worker',p,{})
                self.assertTrue(all('--commit-markers' in c.args[0] for c in execute.call_args_list))

    def test_no_metrics_flag_does_not_launch_companions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);p=save(root,'run',fixture(profile=False))
            a=B.parser().parse_args(['run','--all-rules','--analysis',str(root/'lib'),'--no-cell-metrics'])
            with patch.object(B,'main') as execute:
                B.finish_cell_metrics(a,root,root/'worker',p,{})
                execute.assert_not_called()
            self.assertTrue((p/'cell-ledger.csv').exists())
