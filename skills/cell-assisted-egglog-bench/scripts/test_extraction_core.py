from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parent))
import bench
import extraction_core as core


class ExtractionCore(unittest.TestCase):
    def test_timer_encloses_complete_call_but_not_parse_or_output(self):
        source='''pair<EGraph, vector<EClassId> > res = parse_egglog_json();
extractions = extract_all_fun_roots_tiger(g, roots);
output_egglog(g, extractions);'''
        output=core.instrument_main(source)
        ordered=['parse_egglog_json();','extraction_start =','extract_all_fun_roots_tiger(g, roots);',
                 'extraction_end =','output_egglog(g, extractions);','fflush(stdout);','TIGER_EXTRACTION_TIMING']
        positions=[output.index(s) for s in ordered]
        self.assertEqual(positions,sorted(positions))
        self.assertEqual(output.count('extract_all_fun_roots_tiger(g, roots);'),1)
        with self.assertRaises(ValueError):core.instrument_main(source+source)
        with self.assertRaises(ValueError):core.instrument_main(source.replace('roots);','other);'))

    def test_phase_print_removal_is_specific_and_fail_closed(self):
        line='    std::fprintf(stderr, "TIGER_COST_PHASE example");\n'
        source='keep();\n'+line+'work();\n'+line+'finish();\n'
        self.assertEqual(core.remove_phase_prints(source),'keep();\nwork();\nfinish();\n')
        with self.assertRaises(ValueError):core.remove_phase_prints(line)

    def test_missing_negative_or_impossible_timing_rejected(self):
        row=dict(returncode=0,timeout=False,wall_seconds=10,stats=[
            dict(read_parse_seconds=2,algorithm_seconds=3,serialize_write_seconds=1)])
        self.assertEqual(core.timing(row)['algorithm_seconds'],3)
        for changes in [dict(stats=[]),dict(stats=row['stats']*2),dict(wall_seconds=5),
                        dict(returncode=1),dict(timeout=True),
                        dict(stats=[dict(read_parse_seconds=2,algorithm_seconds=-1,serialize_write_seconds=1)])]:
            with self.assertRaises(ValueError):core.timing(dict(row,**changes))

    def test_dry_run_does_not_build_or_change_old_suite(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            result=bench.main(['--dry-run','extraction-core','--suite',str(root/'suite'),
                              '--checkout',str(root/'checkout'),'--build-dir',str(root/'build'),
                              '--output',str(root/'output')])
            self.assertEqual(result,0)
            self.assertEqual(list(root.iterdir()),[])


if __name__=='__main__':unittest.main()
