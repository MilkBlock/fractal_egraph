"""Multi-source contexts, mutable ports, aliases, visibility and source Runs."""
import json,subprocess,tempfile,unittest
from pathlib import Path
from eggfuse.engine.cell_runtime import prepare,execute
from eggfuse.ir.cell_runtime import build
from eggfuse.engine.cpu import compare_native
from experiments.cell_runtime.reference_views_test import FRONTEND,ENGINE
from experiments.cell_runtime.presearch_bench import SETTINGS
from experiments.cell_runtime.check import CASES

BASE='''(datatype E (N i64) (R E E) (S E E) (Out E E i64))
(function Cost (E) i64 :merge new)
(ruleset a) (ruleset change) (ruleset producer)
(rule ((R x y) (S y z) (= c (Cost z)) (> c 0)) ((Out x z c)) :ruleset a)
(R (N 0) (N 1)) (R (N 0) (N 2))
(S (N 1) (N 3)) (S (N 2) (N 4))
(set (Cost (N 3)) 9) (set (Cost (N 4)) 7)
'''

class RetainedTests(unittest.TestCase):
    def setUp(self):self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
    def tearDown(self):self.tmp.cleanup()
    def pair(self,source,native=True,extra=None):
        src=self.root/'source.egg';src.write_text(source);core=prepare(src,FRONTEND);p=build(core)
        sets=','.join(s['name'] for s in p['recipe']['rulesets']);reports=[]
        env={**SETTINGS,'CELL_CPU_LOCAL_SAT':'0','CELL_CPU_RETAINED_SETS':sets,**(extra or {})}
        for flag in ('0','1'):
            reports.append(execute(p,self.root/flag,ENGINE,audit=True,environment={**env,'CELL_CPU_RETAINED_CONTEXTS':flag}))
        for name in ('state.json','stages.json','binding-audit.json'):
            self.assertEqual(json.loads((self.root/'0/runtime'/name).read_text()),json.loads((self.root/'1/runtime'/name).read_text()),name)
        if native:
            subprocess.run([str(FRONTEND),str(src),'--native-full','--export-to',str(self.root/'native')],check=True,capture_output=True)
            compare_native(core,json.loads((self.root/'1/runtime/state.json').read_text()),json.loads((self.root/'native/native-state.json').read_text()))
        return p,reports,env
    def test_independent_sources_and_function_update(self):
        _,(_,r),_=self.pair(BASE+'''(rule () ((set (Cost (N 3)) 2)) :ruleset change)
(run-schedule (seq (run a) (run change) (run a)))''')
        s=r['retained_contexts']['stats']
        self.assertGreater(s['multi_producer_bindings'],0)
        self.assertGreater(s['domains_reused'],0)
        self.assertGreater(s['function_domains_refreshed'],0)
    def test_empty_port_can_arrive_later(self):
        source=BASE.replace('(set (Cost (N 3)) 9)','')+'''
(rule () ((set (Cost (N 3)) 4)) :ruleset change)
(run-schedule (seq (run a) (run change) (run a)))'''
        _,(_,r),_=self.pair(source)
        self.assertGreater(r['retained_contexts']['stats']['waiting_domains'],0)
    def test_union_opens_new_correlated_alternatives(self):
        _,(_,r),_=self.pair(BASE+'''
(rule () ((union (N 1) (N 2))) :ruleset change)
(run-schedule (seq (run a) (run change) (run a)))''')
        self.assertGreater(r['retained_contexts']['stats']['union_refreshes'],0)
    def test_union_parent_congruence_and_multiple_candidates(self):
        self.pair('''(datatype E (N i64) (F E) (R E E) (S E E) (Out E E)) (ruleset a) (ruleset change)
(rule ((R x y) (S y z)) ((Out x z)) :ruleset a)
(R (N 0) (F (N 1))) (S (F (N 2)) (N 3)) (S (F (N 2)) (N 4))
(rule () ((union (N 1) (N 2))) :ruleset change)
(run-schedule (seq (run a) (run change) (run a)))''')
    def test_custom_merge_waits_for_later_union(self):
        self.pair('''(datatype T (Term i64)) (datatype C (Pair T i64) (Choose C C))
(datatype E (N i64) (Out E T i64)) (relation Anchor (E))
(function Best (E) C :merge (Choose old new))
(ruleset a) (ruleset update) (ruleset resolve)
(rule ((Anchor x) (= chosen (Best x)) (= chosen (Pair t cost))) ((Out x t cost)) :ruleset a)
(rule ((= chosen (Choose (Pair ta ca) (Pair tb cb))) (< cb ca)) ((union chosen (Pair tb cb))) :ruleset resolve)
(rule () ((set (Best (N 0)) (Pair (Term 1) 2))) :ruleset update)
(Anchor (N 0)) (set (Best (N 0)) (Pair (Term 0) 9))
(run-schedule (seq (run a) (run update) (run a) (run resolve) (run a)))
(check (Out (N 0) (Term 1) 2))''')
    def test_delete_and_subsume_remove_stale_evidence(self):
        root=self.root
        try:
            for op in ('delete','subsume'):
                self.root=root/op;self.root.mkdir()
                self.pair(BASE+f'''(rule () (({op} (S (N 1) (N 3))) (set (Cost (N 3)) 2)) :ruleset change)
(run-schedule (seq (run a) (run change) (run a)))''')
        finally:self.root=root
    def test_fingerprint_collisions_cannot_hide_changed_values(self):
        self.pair(BASE+'''(rule () ((set (Cost (N 3)) -1) (set (Cost (N 4)) 2)) :ruleset change)
(run-schedule (seq (run a) (run change) (run a)))''',extra={'CELL_CPU_HASH_BITS':'0'})
    def test_index_reset_keeps_contexts_sound(self):
        root=self.root
        try:
            for scope in ('run','table'):
                self.root=root/scope;self.root.mkdir()
                self.pair(BASE+'''(rule () ((set (Cost (N 3)) 2) (union (N 1) (N 2))) :ruleset change)
(run-schedule (seq (run a) (run change) (run a)))''',extra={'CELL_CPU_QUERY_INDEX_SCOPE':scope})
        finally:self.root=root
    def test_naive_effects_still_repeat_in_order(self):
        self.pair('''(datatype E (N i64) (R E) (S E)) (function Count () i64 :merge (+ old new)) (ruleset a)
(rule ((R x) (S x)) ((set (Count) 1)) :ruleset a :naive)
(R (N 0)) (S (N 0)) (run-schedule (repeat 3 (run a))) (check (= (Count) 3))''')
    def test_finite_run_does_not_see_sibling_rhs(self):
        self.pair('''(datatype E (N i64) (P E) (Q E) (R E) (S E)) (ruleset a)
(rule ((P x) (R x)) ((Q x)) :ruleset a)
(rule ((Q x) (R x)) ((S x)) :ruleset a)
(P (N 0)) (R (N 0)) (run-schedule (run a))''')
    def test_repeated_updates_do_not_accumulate_history(self):
        _,(_,r),_=self.pair(BASE+'''(rule ((= c (Cost (N 3))) (> c 0)) ((set (Cost (N 3)) (- c 1))) :ruleset change)
(run-schedule (repeat 12 (seq (run a) (run change))))''')
        c=r['retained_contexts'];self.assertGreater(c['stats']['retired_contexts'],0)
        self.assertLess(c['live_contexts'],c['stats']['contexts_created'])
        self.assertEqual(c['live_contexts'],c['stats']['live_contexts'])
        self.assertEqual(c['live_branches'],c['stats']['live_branches'])
        self.assertEqual(c['context_slots']-c['free_context_slots'],c['live_contexts'])
        self.assertLessEqual(c['context_slots'],c['stats']['peak_contexts'])
    def test_union_changes_a_fixed_global_input(self):
        self.pair('''(datatype E (N i64) (R E) (S E) (Out E)) (ruleset a) (ruleset change)
(let b (N 0)) (let fixed (N 1)) (R b) (S b)
(rule ((R fixed) (S fixed)) ((Out fixed)) :ruleset a)
(rule () ((union fixed b)) :ruleset change)
(run-schedule (seq (run a) (run change) (run a))) (check (Out b))''')
    def test_recycled_contexts_do_not_receive_retired_subscriptions(self):
        actions=['(set (Cost (N 3)) -1)','(delete (S (N 1) (N 3)))',
            '(S (N 1) (N 5)) (set (Cost (N 5)) 2)','(union (N 1) (N 2))',
            '(set (Cost (N 4)) 1)','(delete (R (N 0) (N 2)))',
            '(R (N 0) (N 1))','(set (Cost (N 5)) 7)']
        source=BASE+''.join(f'(ruleset c{i}) (rule () ({a}) :ruleset c{i})\n' for i,a in enumerate(actions))
        source+='(run-schedule (seq (run a) '+''.join(f'(run c{i}) (run a) ' for i in range(len(actions)))+'))'
        _,(_,r),_=self.pair(source)
        self.assertGreater(r['retained_contexts']['stats']['retired_contexts'],0)
    def test_checkpoint_rebuilds_contexts_without_replaying_old_effects(self):
        p,_,env=self.pair(BASE+'''(rule () ((set (Cost (N 3)) 2)) :ruleset change)
(run-schedule (seq (run a) (run change) (run a)))''')
        env={**env,'CELL_CPU_RETAINED_CONTEXTS':'1'}
        execute(p,self.root/'pause',ENGINE,checkpoint_after=1,environment=env)
        execute(p,self.root/'resume',ENGINE,resume=self.root/'pause/runtime/checkpoint.json',environment=env)
        self.assertEqual(json.loads((self.root/'1/runtime/state.json').read_text()),json.loads((self.root/'resume/runtime/state.json').read_text()))
    def test_existing_semantics(self):
        root=self.root
        try:
            for name,source in CASES.items():
                with self.subTest(case=name):self.root=root/name;self.root.mkdir();self.pair(source,native=False)
        finally:self.root=root

if __name__=='__main__':unittest.main()
