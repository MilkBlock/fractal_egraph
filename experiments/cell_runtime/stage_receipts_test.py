"""Direct query/action receipts, validated after the original repair boundary."""
import unittest
from experiments.cell_runtime import stage_spine_test as st

class StageReceiptTests(st.StageSpineTests):
    MODES=({'CELL_CPU_STAGE_SPINE':'1','CELL_CPU_STAGE_SPINE_PARTIAL':'1','CELL_CPU_STAGE_SPINE_SHARED_ENTRIES':'1','CELL_CPU_STAGE_SPINE_RECEIPTS':'0'},
           {'CELL_CPU_STAGE_SPINE':'1','CELL_CPU_STAGE_SPINE_PARTIAL':'1','CELL_CPU_STAGE_SPINE_SHARED_ENTRIES':'1','CELL_CPU_STAGE_SPINE_RECEIPTS':'1'})
    def test_new_and_reused_results_need_no_capture_lookup(self):
        _,(before,after)=self.pair(st.CHAIN+'(P (N 0)) (run-schedule (seq (run a) (run b) (run c)))')
        s=after['stage_spine']['stats']
        self.assertGreater(before['stage_spine']['stats']['capture_reads'],0)
        self.assertEqual(s['capture_reads'],0)
        self.assertGreater(s['receipt_hits_by_source']['query'],0)
        self.assertGreater(s['receipt_hits_by_source']['effect'],0)
    def test_same_key_constructor_coalescence_forwards_receipt(self):
        _,(_,after)=self.pair('''(datatype E (N i64) (Seed E) (F E) (Known E) (Out E))
(ruleset a) (ruleset b)
(rule ((Seed x)) ((F (N 0)) (F x) (Known x) (union x (N 0))) :ruleset a)
(rule ((F x) (Known x)) ((Out x)) :ruleset b)
(Seed (N 1)) (F (N 0))
(run-schedule (seq (run a) (run b))) (check (Out (N 0)))''')
        s=after['stage_spine']['stats'];self.assertGreater(s['receipt_redirects'],0)
        self.assertEqual(s['capture_reads'],0)
    def test_function_receipt_reads_actual_merge_and_forwarded_key(self):
        _,(_,after)=self.pair('''(datatype E (N i64) (Seed E) (P E) (Out E i64))
(function Cost (E) i64 :merge (+ old new)) (ruleset a) (ruleset b)
(rule ((= fixed (Cost (N 0))) (Seed x) (= c (Cost x))) ((P x) (set (Cost x) 2) (union x (N 0))) :ruleset a)
(rule ((P x) (= c (Cost x))) ((Out x c)) :ruleset b)
(Seed (N 1)) (set (Cost (N 0)) 5) (set (Cost (N 1)) 3)
(run-schedule (seq (run a) (run b))) (check (Out (N 0) 10))''')
        self.assertGreater(after['stage_spine']['stats']['receipt_redirects'],0)
    def test_deleted_and_reinserted_key_uses_current_witness(self):
        _,(_,after)=self.pair('''(datatype E (N i64) (Seed E) (P E) (Out E i64))
(function Cost (E) i64 :merge new) (ruleset a) (ruleset b)
(rule ((Seed x)) ((P x) (set (Cost x) 1)) :ruleset a)
(rule () ((delete (Cost (N 0))) (set (Cost (N 0)) 7)) :ruleset a)
(rule ((P x) (= n (Cost x))) ((Out x n)) :ruleset b)
(Seed (N 0)) (run-schedule (seq (run a) (run b))) (check (Out (N 0) 7))''')
        self.assertGreater(after['stage_spine']['stats']['receipt_rejections'],0)
        self.assertGreater(after['stage_spine']['stats']['capture_reads'],0)
    def test_hidden_result_is_not_published_as_visible(self):
        self.pair(st.CHAIN+'''(ruleset hide)
(rule () ((subsume (Known (N 0)))) :ruleset hide)
(run-schedule (seq (run a) (run hide) (run b) (run c)))''')
    def test_witness_free_instruction_reuse_keeps_exact_fallback(self):
        self.MODES=tuple({**m,'CELL_CPU_KNOWN_RESULTS':'static'} for m in self.MODES)
        self.pair('''(datatype E (N i64) (Seed E) (P E) (Q E) (Out E))
(ruleset a) (ruleset b)
(rule ((Seed x)) ((P x) (Q (P x))) :ruleset a)
(rule ((P x) (Q (P x))) ((Out x)) :ruleset b)
(Seed (N 0)) (run-schedule (seq (run a) (run b)))''')
if __name__=='__main__':unittest.main()
