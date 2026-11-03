"""Shared multi-atom Spine providers and independently scheduled consumers."""
import json,unittest
from eggfuse.engine.cell_runtime import execute
from experiments.cell_runtime import retained_test as rt

SOURCE='''(datatype E (N i64) (R E E) (S E E) (T E E) (Out E E i64))
(function Cost (E) i64 :merge new)
(ruleset left) (ruleset right) (ruleset deep) (ruleset mutate)
(rule ((R x y) (S y z) (= c (Cost z)) (> c 0)) ((Out x z c)) :ruleset left)
(rule ((R aa bb) (S bb cc) (= price (Cost cc)) (< price 10)) ((Out aa cc (+ price 10))) :ruleset right)
(rule ((R x y) (S y z) (T z w) (= c (Cost w))) ((Out x w (+ c 20))) :ruleset deep)
(rule ((R aa bb) (S bb cc) (T cc dd) (= price (Cost dd))) ((Out aa dd (+ price 30))) :ruleset deep)
(R (N 0) (N 1)) (R (N 0) (N 2))
(S (N 1) (N 3)) (S (N 2) (N 4))
(T (N 3) (N 5)) (T (N 4) (N 6))
(set (Cost (N 3)) 9) (set (Cost (N 4)) 7) (set (Cost (N 5)) 8) (set (Cost (N 6)) 6)
'''

class PrefixTests(unittest.TestCase):
    setUp=rt.RetainedTests.setUp
    tearDown=rt.RetainedTests.tearDown
    def pair(self,source,extra=None):
        return rt.RetainedTests.pair(self,source,extra={'CELL_CPU_SPINE_PREFIXES':'1',**(extra or {})})
    def test_nested_prefixes_reuse_complete_correlated_bindings(self):
        _,(_,r),_=self.pair(SOURCE+'''(rule () ((set (Cost (N 3)) 2)) :ruleset mutate)
(run-schedule (seq (run left) (run deep) (run mutate) (run right) (run left) (run deep)))''')
        p=r['retained_contexts']['spine_prefixes']
        self.assertTrue(any(x['parent'] is not None for x in p['plans']))
        self.assertGreater(max(x['atoms'] for x in p['plans']),2)
        self.assertGreater(p['stats']['provider_hits'],0)
        self.assertGreater(p['stats']['consumer_calls'],0)
        self.assertEqual(r['retained_contexts']['live_contexts'],r['retained_contexts']['stats']['live_contexts'])
        self.assertEqual(r['retained_contexts']['context_slots']-r['retained_contexts']['free_context_slots'],r['retained_contexts']['live_contexts'])
    def test_late_prefix_row_and_slow_consumer(self):
        self.pair(SOURCE.replace('(S (N 1) (N 3))','')+'''
(rule () ((S (N 1) (N 3)) (set (Cost (N 3)) 2)) :ruleset mutate)
(run-schedule (seq (run left) (run mutate) (run deep) (run left) (run right)))''')
    def test_union_then_subsume_retires_correlated_suffixes(self):
        source=SOURCE+'''(ruleset hide)
(rule () ((union (N 1) (N 2))) :ruleset mutate)
(rule () ((subsume (S (N 1) (N 3))) (set (Cost (N 4)) 1)) :ruleset hide)
(run-schedule (seq (run left) (run deep) (run mutate) (run right) (run deep) (run hide) (run left) (run deep)))'''
        _,(_,r),_=self.pair(source,extra={'CELL_CPU_HASH_BITS':'0'})
        self.assertGreater(r['retained_contexts']['spine_prefixes']['stats']['prefix_rows_retired'],0)
    def test_alias_and_literal_filters_are_part_of_prefix_identity(self):
        self.pair(SOURCE+'''
(rule ((R x x) (S x y)) ((Out x y 100)) :ruleset left)
(rule ((R x x) (S x y)) ((Out x y 101)) :ruleset right)
(rule ((R x y) (S y z) (= 9 (Cost z))) ((Out x z 102)) :ruleset right)
(rule () ((union (N 0) (N 1))) :ruleset mutate)
(run-schedule (seq (run left) (run right) (run mutate) (run right) (run left)))''')
    def test_fixed_identity_literals_follow_union(self):
        _,(_,r),_=self.pair('''(datatype E (N i64) (R E) (S E) (Out E i64))
(let other (N 0)) (let fixed (N 1)) (R other) (S other)
(ruleset left) (ruleset right) (ruleset mutate)
(rule ((R fixed) (S fixed)) ((Out fixed 0)) :ruleset left)
(rule ((R fixed) (S fixed)) ((Out fixed 1)) :ruleset right)
(rule () ((union fixed other)) :ruleset mutate)
(run-schedule (seq (run left) (run mutate) (run right) (run left)))''')
        self.assertTrue(r['retained_contexts']['spine_prefixes']['plans'])
    def test_shared_prefix_does_not_change_naive_or_finite_runs(self):
        self.pair('''(datatype E (N i64) (R E) (S E) (Out E))
(function Count () i64 :merge (+ old new)) (ruleset left) (ruleset right)
(rule ((R x) (S x)) ((Out x)) :ruleset left)
(rule ((R x) (S x)) ((set (Count) 1)) :ruleset right :naive)
(R (N 0)) (S (N 0))
(run-schedule (repeat 3 (seq (run left) (run right)))) (check (= (Count) 3))''')
    def test_finite_run_cannot_consume_sibling_effect(self):
        self.pair('''(datatype E (N i64) (R E) (S E) (T E) (Out E)) (ruleset once)
(rule ((R x) (S x)) ((T x)) :ruleset once)
(rule ((R x) (S x) (T x)) ((Out x)) :ruleset once)
(R (N 0)) (S (N 0)) (run-schedule (run once))''')
    def test_checkpoint_resumes_without_duplicate_old_bindings(self):
        p,_,env=self.pair(SOURCE+'''(rule () ((set (Cost (N 3)) 1)) :ruleset mutate)
(run-schedule (seq (run left) (run mutate) (run right) (run deep) (run left)))''')
        env={**env,'CELL_CPU_RETAINED_CONTEXTS':'1'}
        execute(p,self.root/'pause',rt.ENGINE,checkpoint_after=1,environment=env)
        execute(p,self.root/'resume',rt.ENGINE,resume=self.root/'pause/runtime/checkpoint.json',environment=env)
        self.assertEqual(json.loads((self.root/'1/runtime/state.json').read_text()),json.loads((self.root/'resume/runtime/state.json').read_text()))
    def test_removed_binding_reappears_before_slow_consumer_runs(self):
        self.pair(SOURCE+'''(ruleset restore)
(rule () ((delete (S (N 1) (N 3)))) :ruleset mutate)
(rule () ((S (N 1) (N 3))) :ruleset restore)
(run-schedule (seq (run right) (run left) (run mutate) (run left) (run restore) (run left) (run right) (run deep)))''')
    def test_index_reset_reacquires_shared_prefix(self):
        self.pair(SOURCE+'''(rule () ((set (Cost (N 3)) 1)) :ruleset mutate)
(run-schedule (seq (run left) (run right) (run mutate) (run deep) (run left)))''',extra={'CELL_CPU_QUERY_INDEX_SCOPE':'run'})

if __name__=='__main__':unittest.main()
