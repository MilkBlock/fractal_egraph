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


if __name__=='__main__':unittest.main()
