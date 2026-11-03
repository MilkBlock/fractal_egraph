"""Exact anchor predicates shared across independently wired consumers."""
import unittest
from experiments.cell_runtime import stage_partial_test as pt

class StageSharedTests(pt.StagePartialTests):
    MODES=({'CELL_CPU_STAGE_SPINE':'1','CELL_CPU_STAGE_SPINE_PARTIAL':'1','CELL_CPU_STAGE_SPINE_SHARED_ENTRIES':'0','CELL_CPU_STAGE_SPINE_RECEIPTS':'0'},
           {'CELL_CPU_STAGE_SPINE':'1','CELL_CPU_STAGE_SPINE_PARTIAL':'1','CELL_CPU_STAGE_SPINE_SHARED_ENTRIES':'1','CELL_CPU_STAGE_SPINE_RECEIPTS':'0'})
    def test_mixed_known_and_unknown_producers_keep_covered_entries(self):
        _,(before,after)=self.pair(pt.CHAIN+'''(P (N 1)) (Known (N 1))
(run-schedule (seq (run a) (run b) (run c))) (check (R (N 0)) (R (N 1)))''')
        self.assertEqual(before['stage_spine']['stats']['bindings'],after['stage_spine']['stats']['bindings'])
        self.assertGreater(after['stage_spine']['stats']['partial_queries'],0)
    def test_renamed_consumers_reuse_one_exact_entry_check(self):
        _,(before,after)=self.pair('''(datatype E (N i64) (Seed E) (P E) (Q E) (Out E i64))
(ruleset a) (ruleset b)
(rule ((Seed x)) ((P x) (Q x)) :ruleset a)
(rule ((P x) (Q x)) ((Out x 1)) :ruleset b)
(rule ((P renamed) (Q renamed)) ((Out renamed 2)) :ruleset b)
(rule ((P other) (Q other)) ((Out other 3)) :ruleset b)
(Seed (N 0)) (run-schedule (seq (run a) (run b)))''')
        s=after['stage_spine']['stats']
        self.assertGreater(s['shared_entry_cross_rule_hits'],0)
        self.assertLess(s['pivot_candidates'],before['stage_spine']['stats']['pivot_candidates'])
        self.assertLess(s['delta_rows_examined'],before['stage_spine']['stats']['delta_rows_examined'])
    def test_shared_residual_entries_without_any_producer_context(self):
        _,(before,after)=self.pair('''(datatype E (N i64) (Edge E E) (Out E E i64))
(ruleset consume)
(rule ((Edge x x)) ((Out x x 1)) :ruleset consume)
(rule ((Edge left right)) ((Out left right 2)) :ruleset consume)
(rule ((Edge renamed other)) ((Out renamed other 3)) :ruleset consume)
(Edge (N 0) (N 1)) (Edge (N 0) (N 0))
(run-schedule (run consume))''')
        self.assertEqual(after['stage_spine']['stats']['contexts_created'],0)
        self.assertGreater(after['stage_spine']['stats']['shared_entry_cross_rule_hits'],0)
        def visits(r):
            return r['stats']['residual_candidates']+r['stats']['direct_candidates']+r['multi_stage']['stats']['merge_candidate_rows']+r['stage_spine']['stats']['pivot_candidates']
        self.assertLess(visits(after),visits(before))
    def test_literals_aliases_and_column_positions_are_part_of_identity(self):
        self.pair('''(datatype E (N i64) (Seed E) (Edge E E) (Out E E i64))
(ruleset a) (ruleset b)
(rule ((Seed x)) ((Edge x (N 1)) (Edge x x)) :ruleset a)
(rule ((Edge x x)) ((Out x x 1)) :ruleset b)
(rule ((Edge left right)) ((Out left right 2)) :ruleset b)
(rule ((Edge (N 0) v)) ((Out (N 0) v 3)) :ruleset b)
(rule ((Edge (N 1) v)) ((Out (N 1) v 4)) :ruleset b)
(rule ((Edge v (N 0))) ((Out v (N 0) 5)) :ruleset b)
(Seed (N 0)) (run-schedule (seq (run a) (run b)))''')
if __name__=='__main__':unittest.main()
