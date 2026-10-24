import copy
import json
from pathlib import Path
import tempfile
import unittest
import hotspots


class Hotspots(unittest.TestCase):
    def test_ranking_uses_visits_not_successful_matches(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'report.json'
            stats=lambda visits,m:dict(depth_work=[[visits]],eligible_seeds=5,emitted_bindings=m)
            p.write_text(json.dumps(dict(gpu=dict(cells=dict(profile=True),join_depth_fields=['live_rows'],rules=[
                dict(rule='empty-hot',ruleset='r',stats=stats(100,0)),dict(rule='productive',ruleset='r',stats=stats(3,10))]))))
            r=hotspots.summarize(p)
            self.assertEqual(r['gpu'][0]['rule'],'empty-hot')
            self.assertEqual(r['measured_gpu_rows'],103)

    def test_cell_bindings_are_a_subset_and_not_double_counted(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);a=root/'a';b=root/'b';a.mkdir();b.mkdir()
            gpu=dict(runtime=dict(run_seconds=2),cells=dict(profile=False),query_timing_profile=False,audit=False,
                     rules=[dict(rule='q',ruleset='s',stats=dict(emitted_bindings=10,cell_bindings=0))],
                     saturation_diagnostics=dict(programs=[]))
            report=dict(same_commit_verification=dict(exact_rows_values_equivalence_visibility=True),gpu=gpu,schedule=[],assisted_rulesets=[])
            workflow=dict(snapshot_sha256='s',binary_sha256='b',worker_sha256='w',policy='stable',
                          inference_identity=dict(boundary_programs=None,joint=True,all_rules=True))
            (a/'report.json').write_text(json.dumps(report));(a/'workflow.json').write_text(json.dumps(workflow))
            after=copy.deepcopy(report);after['gpu']['rules'][0]['stats']['cell_bindings']=10
            after['gpu']['runtime']['run_seconds']=1
            workflow['inference_identity']['boundary_programs']='file-sha'
            (b/'report.json').write_text(json.dumps(after));(b/'workflow.json').write_text(json.dumps(workflow))
            self.assertEqual(hotspots.compare(a,b)['saved_fraction'],.5)
            after['gpu']['rules'][0]['stats']['emitted_bindings']=9
            (b/'report.json').write_text(json.dumps(after))
            with self.assertRaisesRegex(ValueError,'binding count'):hotspots.compare(a,b)

    def test_extension_preserves_old_plans_and_total_bindings(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);a=root/'a';b=root/'b';a.mkdir();b.mkdir()
            program=lambda name:dict(kind='boundary_query',ruleset='s',rule=name,seed='Root',accesses=['Key'])
            gpu=dict(runtime=dict(run_seconds=2),cells=dict(profile=False),query_timing_profile=False,audit=False,
                rules=[dict(rule=q,ruleset='s',stats=dict(emitted_bindings=3)) for q in ['old','new']],
                saturation_diagnostics=dict(programs=[program('old')]))
            report=dict(same_commit_verification=dict(exact_rows_values_equivalence_visibility=True),
                        gpu=gpu,schedule=[],assisted_rulesets=[])
            workflow=dict(snapshot_sha256='s',binary_sha256='b',worker_sha256='w',policy='stable',
                          inference_identity=dict(boundary_programs='old-library',joint=True,all_rules=True))
            (a/'report.json').write_text(json.dumps(report));(a/'workflow.json').write_text(json.dumps(workflow))
            after=copy.deepcopy(report);after['gpu']['saturation_diagnostics']['programs'].append(program('new'))
            workflow['inference_identity']['boundary_programs']='new-library'
            (b/'report.json').write_text(json.dumps(after));(b/'workflow.json').write_text(json.dumps(workflow))
            with self.assertRaisesRegex(ValueError,'off/on'):hotspots.compare(a,b)
            self.assertEqual(len(hotspots.compare(a,b,extension=True)['added_programs']),1)
            # An optional extraction checker is not a different inference binary.
            original=copy.deepcopy(workflow)
            for path,extra in [(a,{}),(b,{'tiger':'hash'})]:
                w=copy.deepcopy(original);w['inference_identity']['binary']='inference-hash'
                w['binary_sha256']=dict(inference='inference-hash',**extra)
                (path/'workflow.json').write_text(json.dumps(w))
            self.assertEqual(len(hotspots.compare(a,b,extension=True)['added_programs']),1)
            w['inference_identity']['binary']='different'
            (b/'workflow.json').write_text(json.dumps(w))
            with self.assertRaisesRegex(ValueError,'inference binary'):hotspots.compare(a,b,extension=True)
            w['inference_identity']['binary']='inference-hash'
            (b/'workflow.json').write_text(json.dumps(w))
            after['gpu']['saturation_diagnostics']['programs'][0]['seed']='DifferentRoot'
            (b/'report.json').write_text(json.dumps(after))
            with self.assertRaisesRegex(ValueError,'preserve'):hotspots.compare(a,b,extension=True)

    def test_layout_comparison_preserves_family_and_reports_cpu_work(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);a=root/'a';b=root/'b';a.mkdir();b.mkdir()
            program=dict(kind='boundary_query',ruleset='s',rule='q',seed='If',accesses=[],index_reads=3,candidate_rows=4,seed_evaluations=2)
            gpu=dict(runtime=dict(run_seconds=2),cells=dict(profile=False),query_timing_profile=False,audit=False,
                rules=[dict(rule='q',ruleset='s',stats=dict(emitted_bindings=3))],saturation_diagnostics=dict(programs=[program]))
            report=dict(same_commit_verification=dict(exact_rows_values_equivalence_visibility=True),gpu=gpu,schedule=[],assisted_rulesets=[])
            workflow=dict(snapshot_sha256='s',binary_sha256='b',worker_sha256='w',policy='stable',inference_identity=dict(boundary_programs='p',joint=True,all_rules=True))
            for path in [a,b]:
                (path/'report.json').write_text(json.dumps(report));(path/'workflow.json').write_text(json.dumps(workflow))
            report['gpu']['saturation_diagnostics']['programs'][0]['seed']='HasType'
            report['gpu']['saturation_diagnostics']['programs'][0]['candidate_rows']=400
            (b/'report.json').write_text(json.dumps(report))
            result=hotspots.compare(a,b,layouts=True)
            self.assertEqual([w['row_visits'] for w in result['cpu_work']],[4,400])
            report['gpu']['saturation_diagnostics']['port_reuse']={'enabled':False}
            report['gpu']['saturation_diagnostics']['programs'][0]['cache_ports']=False
            (b/'report.json').write_text(json.dumps(report))
            hotspots.compare(a,b,layouts=True)  # selected boundary cache policy may differ
            with self.assertRaisesRegex(ValueError,'preserve'):hotspots.compare(a,b,extension=True)
            report['gpu']['saturation_diagnostics']['programs'][0]['rule']='other'
            (b/'report.json').write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError,'query family'):hotspots.compare(a,b,layouts=True)
            report['gpu']['saturation_diagnostics']['programs'][0]['rule']='q'
            protected=dict(program,rule='protected',kind='dataflow',cache_ports=True)
            old=json.loads((a/'report.json').read_text());old['gpu']['saturation_diagnostics']['programs'].append(protected)
            report['gpu']['saturation_diagnostics']['programs'].append(dict(protected,cache_ports=False))
            (a/'report.json').write_text(json.dumps(old));(b/'report.json').write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError,'existing Cell'):hotspots.compare(a,b,layouts=True)


if __name__=='__main__':unittest.main()
