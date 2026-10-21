import json
from pathlib import Path
import tempfile
import unittest
import marker_pass3
import bench


class MarkerPass3(unittest.TestCase):
    def test_cli_has_explicit_source_level_native_mode(self):
        a=bench.parser().parse_args(['marker-pass3','--analysis','a','--reuse-controls','b','--output','c'])
        self.assertEqual(a.variant,'published')
        self.assertFalse(a.profile)

    def test_unreported_is_not_a_cache_miss_or_fake_zero_measurement(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);program=p/'program';run=p/'run';program.mkdir();run.mkdir()
            (program/'markers.json').write_text(json.dumps(dict(markers=[dict(name='M',cells=['state-0'],occurrences=[dict(rule=0),dict(rule=1)])])))
            (program/'source-ast.json').write_text(json.dumps(dict(rules=[dict(ordinal=0,name='a',ruleset='r'),dict(ordinal=1,name='b',ruleset='r')])))
            (run/'report.json').write_text(json.dumps(dict(run_report=dict(num_matches_per_rule={'a':0}),marker_rows={'M':0},verification={'exact':True})))
            s=marker_pass3.summarize(program,run)
            self.assertEqual(s['successful_consumer_bindings'],0)
            self.assertEqual(s['rows'][0]['logical_matches'],0)
            self.assertIsNone(s['rows'][1]['logical_matches'])
            self.assertIsNone(s['extraction_passed'])


if __name__=='__main__':unittest.main()
