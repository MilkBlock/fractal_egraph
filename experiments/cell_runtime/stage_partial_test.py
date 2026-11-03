"""Entry-level complete coverage with ordinary matching of only the gaps."""
import unittest
from experiments.cell_runtime import stage_spine_test as st
CHAIN=st.CHAIN

class StagePartialTests(st.StageSpineTests):
    MODES=({'CELL_CPU_STAGE_SPINE':'1','CELL_CPU_STAGE_SPINE_PARTIAL':'0','CELL_CPU_STAGE_SPINE_SHARED_ENTRIES':'0','CELL_CPU_STAGE_SPINE_RECEIPTS':'0'},
           {'CELL_CPU_STAGE_SPINE':'1','CELL_CPU_STAGE_SPINE_PARTIAL':'1','CELL_CPU_STAGE_SPINE_SHARED_ENTRIES':'0','CELL_CPU_STAGE_SPINE_RECEIPTS':'0'})
    def test_mixed_known_and_unknown_producers_keep_covered_entries(self):
        _,(before,after)=self.pair(CHAIN+'''(P (N 1)) (Known (N 1))
(run-schedule (seq (run a) (run b) (run c))) (check (R (N 0)) (R (N 1)))''')
        s=after['stage_spine']['stats']
        self.assertGreater(s['partial_queries'],0)
        self.assertGreater(s['pivots_skipped'],before['stage_spine']['stats']['pivots_skipped'])
    def test_same_witness_has_supported_and_unsupported_pivots(self):
        _,(_,r)=self.pair('''(datatype E (N i64) (F E) (Seed E) (Out E E E))
(ruleset a) (ruleset b)
(rule ((Seed y)) ((F y)) :ruleset a)
(rule ((= y (F x)) (= z (F y))) ((Out x y z)) :ruleset b)
(F (N 0)) (Seed (F (N 0)))
(run-schedule (seq (run b) (run a) (run b)))
(check (Out (N 0) (F (N 0)) (F (F (N 0)))))''')
        s=r['stage_spine']['stats']
        self.assertGreater(s['partial_queries'],0)
        self.assertGreater(s['unsupported_key_pivots'],0)
    def test_overlapping_answers_do_not_repeat_effectful_rhs(self):
        self.pair('''(datatype E (N i64) (Seed E) (P E) (Q E))
(function Count () i64 :merge (+ old new)) (ruleset a) (ruleset b)
(rule ((Seed x)) ((P x) (Q x)) :ruleset a)
(rule ((P x) (Q x)) ((set (Count) 1)) :ruleset b)
(P (N 1)) (Q (N 1)) (Seed (N 0))
(run-schedule (seq (run a) (run b))) (check (= (Count) 2))''')
if __name__=='__main__':unittest.main()
