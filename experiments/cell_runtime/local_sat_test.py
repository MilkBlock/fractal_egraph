"""Saturation-local recipes: independent effect checks and source boundaries."""
import json,subprocess,tempfile,unittest
from pathlib import Path
from eggfuse.engine.cell_runtime import prepare,execute
from eggfuse.ir.cell_runtime import build
from eggfuse.engine.cpu import compare_native
from experiments.cell_runtime.reference_views_test import FRONTEND,ENGINE
from experiments.cell_runtime.presearch_bench import SETTINGS
from experiments.cell_runtime.check import CASES

CHAIN='''(datatype E (N i64) (P E) (Q E) (R E)) (ruleset s)
(rule ((P x)) ((Q x)) :ruleset s)
(rule ((Q x)) ((R x)) :ruleset s)
(P (N 0))
'''
class LocalSatTests(unittest.TestCase):
    def setUp(self):self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
    def tearDown(self):self.tmp.cleanup()
    def pair(self,source,native=True,markers=(),extra=None):
        src=self.root/'source.egg';src.write_text(source);core=prepare(src,FRONTEND);p=build(core,reference_tables=list(markers));reports=[]
        for flag in ('0','1'):
            reports.append(execute(p,self.root/flag,ENGINE,environment={**SETTINGS,'CELL_CPU_LOCAL_SAT':flag,'CELL_CPU_LOCAL_SAT_AUDIT':'1',**(extra or {})}))
        a,b=reports
        states=[json.loads((self.root/f/'runtime/state.json').read_text()) for f in ('0','1')]
        compare_native(core,states[1],states[0])
        self.assertEqual(len(a['semantic_audit']),len(b['semantic_audit']))
        for x,y in zip(a['semantic_audit'],b['semantic_audit']):
            self.assertEqual({k:v for k,v in x.items() if k!='state'},{k:v for k,v in y.items() if k!='state'})
            compare_native(core,y['state'],x['state'])
        if native:
            subprocess.run([str(FRONTEND),str(src),'--native-full','--export-to',str(self.root/'native')],check=True,capture_output=True)
            compare_native(core,states[1],json.loads((self.root/'native/native-state.json').read_text()))
        return p,reports
    def test_chain_inside_sat_uses_one_context(self):
        _,(a,b)=self.pair(CHAIN+'(run-schedule (saturate s)) (check (R (N 0)))')
        self.assertGreater(b['local_saturation']['stats']['local_bindings'],0)
        self.assertGreater(b['local_saturation']['stats']['audit_bindings'],0)
        self.assertGreater(b['local_saturation']['stats']['seed_receipts'],0)
        self.assertLess(len(b['steps']),len(a['steps']))
    def test_finite_run_has_no_early_visibility(self):
        _,(_,b)=self.pair(CHAIN+'(run-schedule (run s))')
        self.assertEqual(b['local_saturation']['stats']['local_bindings'],0)
        self.assertFalse(any(r['table']=='R' for r in json.loads((self.root/'1/runtime/state.json').read_text())['rows']))
    def test_completion_key_can_ignore_unused_query_witness(self):
        _,(a,b)=self.pair('''(datatype E (N i64) (Seed E i64) (P E i64) (Known E) (R E)) (ruleset s)
(rule ((Seed x n)) ((P x n) (Known x)) :ruleset s)
(rule ((P x n) (Known x)) ((R x)) :ruleset s)
(Seed (N 0) 1) (Seed (N 0) 2)
(run-schedule (saturate s)) (check (R (N 0)))''')
        self.assertGreater(b['local_saturation']['stats']['duplicate_bindings'],0)
        self.assertGreater(b['local_saturation']['stats']['seed_receipts'],0)
        self.assertLess(b['stats']['bindings'],a['stats']['bindings'])
    def test_partial_lhs_primitive_retains_its_parameters(self):
        _,(_,b)=self.pair('''(datatype E (N i64) (Seed E i64) (P E i64) (R E)) (ruleset s)
(rule ((Seed x n)) ((P x n)) :ruleset s)
(rule ((P x n) (= q (/ 1 n))) ((R x)) :ruleset s)
(Seed (N 0) 1) (Seed (N 0) 0)
(run-schedule (saturate s)) (check (R (N 0)))''')
        self.assertGreater(b['local_saturation']['stats']['guard_rejections'],0)
    def test_unused_failing_rhs_cannot_be_hidden_by_completion(self):
        source='''(datatype E (N i64) (Seed E i64) (P E i64) (R E)) (ruleset s)
(rule ((Seed x n)) ((P x n)) :ruleset s)
(rule ((P x n)) ((let unused (/ 1 n)) (R x)) :ruleset s)
(Seed (N 0) 1) (Seed (N 0) 0) (run-schedule (saturate s))'''
        src=self.root/'source.egg';src.write_text(source);p=build(prepare(src,FRONTEND))
        for flag in ('0','1'):
            with self.assertRaises(subprocess.CalledProcessError):
                execute(p,self.root/flag,ENGINE,environment={**SETTINGS,'CELL_CPU_LOCAL_SAT':flag})
            self.assertIn('RHS primitive failed: /',(self.root/flag/'runtime.log').read_text())
    def test_inert_marker_prelude_is_same_logical_scope(self):
        _,(a,b)=self.pair('''(datatype E (N i64) (P E) (Q E) (R E))
(function View (E) Unit :merge old) (ruleset publish) (ruleset s)
(rule ((P x)) ((set (View x) ())) :ruleset publish)
(rule ((View x)) ((Q x)) :ruleset s)
(rule ((Q x)) ((R x)) :ruleset s)
(P (N 0)) (run-schedule (saturate (seq (saturate publish) (run s))))
(check (R (N 0)))''',markers=['View'])
        self.assertGreaterEqual(b['local_saturation']['stats']['max_chain'],3)
        self.assertLess(len(b['steps']),len(a['steps']))
    def test_independent_lane_can_coexist_with_ordered_merge(self):
        source=CHAIN.replace('(ruleset s)','(function F () i64 :merge new) (relation T ()) (ruleset s)')+'''
(rule ((T)) ((set (F) 1) (set (F) 2)) :ruleset s)
(T) (run-schedule (saturate s)) (check (= (F) 2) (R (N 0)))'''
        _,(_,b)=self.pair(source)
        self.assertGreater(b['local_saturation']['stats']['local_bindings'],0)
    def test_ordered_consumer_dependency_is_a_barrier(self):
        source=CHAIN.replace('(ruleset s)','(function F () i64 :merge new) (ruleset s)')+'''
(rule ((R x)) ((set (F) 2)) :ruleset s)
(run-schedule (saturate s)) (check (= (F) 2))'''
        _,(a,b)=self.pair(source)
        self.assertEqual(b['local_saturation']['stats']['local_bindings'],0)
        self.assertEqual(len(a['steps']),len(b['steps']))
    def test_new_union_binding_reenters_normal_delta(self):
        self.pair('''(datatype E (N i64) (A E) (B E) (Pair E E) (Seen E)) (ruleset s)
(rule ((= a (A x))) ((union a (B x))) :ruleset s)
(rule ((Pair z z) (= z (A x))) ((Seen x)) :ruleset s)
(Pair (A (N 0)) (B (N 0)))
(run-schedule (saturate s)) (check (Seen (N 0)))''')
    def test_receipts_follow_repaired_representatives(self):
        _,(_,b)=self.pair('''(datatype E (N i64) (P E) (Q E) (R E) (Other E)) (ruleset s)
(rule ((P x)) ((Q x)) :ruleset s)
(rule ((Q x)) ((R x)) :ruleset s)
(rule ((R x) (Other y)) ((union x y)) :ruleset s)
(Other (N 0)) (P (N 1))
(run-schedule (saturate s)) (check (= (N 0) (N 1)) (R (N 0)))''')
        self.assertGreater(b['local_saturation']['stats']['canonicalized_receipts'],0)
        self.assertGreater(b['local_saturation']['stats']['seed_receipts'],0)
    def test_receipt_can_stop_an_inner_join(self):
        source='''(datatype E (N i64) (Seed E E E) (P E E) (Q E E) (K E) (R E E)) (ruleset s)
(rule ((Seed x y z)) ((P x y) (Q y z) (K x) (K z)) :ruleset s)
(rule ((P x y) (Q y z) (K x) (K z)) ((R x z)) :ruleset s)
(Seed (N 0) (N 1) (N 2)) (Seed (N 3) (N 4) (N 5))
(run-schedule (saturate s)) (check (R (N 0) (N 2)) (R (N 3) (N 5)))'''
        root=self.root
        try:
            for mode in ('0','1'):
                self.root=root/mode;self.root.mkdir()
                _,(_,b)=self.pair(source,extra={'CELL_CPU_MULTISTAGE':mode})
                self.assertGreater(b['local_saturation']['stats']['inner_receipts'],0)
                self.assertGreater(b['local_saturation']['stats']['skipped_remaining_table_atoms'],0)
        finally:self.root=root
    def test_cycle_returns_to_fair_global_queue(self):
        _,(_,b)=self.pair('''(datatype E (P i64) (Q i64)) (ruleset s)
(rule ((P n) (< n 3)) ((P (+ n 1))) :ruleset s)
(rule ((P n)) ((Q n)) :ruleset s)
(P 0) (run-schedule (saturate s)) (check (Q 3))''')
        self.assertGreater(b['local_saturation']['stats']['cycle_deferrals'],0)
        self.assertGreater(b['local_saturation']['stats']['local_bindings'],0)
    def test_finite_permutation_reentry_closes_locally(self):
        _,(_,b)=self.pair('''(datatype E (N i64) (P E E)) (ruleset s)
(rule ((P x y)) ((P y x)) :ruleset s)
(P (N 0) (N 1)) (run-schedule (saturate s))''')
        self.assertGreater(b['local_saturation']['stats']['finite_reentries'],0)
    def test_expanding_two_rule_cycle_is_deferred(self):
        _,(_,b)=self.pair('''(datatype E (A i64) (B i64)) (ruleset s)
(rule ((A n) (< n 3)) ((B (+ n 1))) :ruleset s)
(rule ((B n)) ((A n)) :ruleset s)
(A 0) (run-schedule (saturate s)) (check (A 3))''')
        self.assertGreater(b['local_saturation']['stats']['cycle_deferrals'],0)
    def test_forwarded_query_slots_cannot_hide_expansion(self):
        _,(_,b)=self.pair('''(datatype E (Root i64) (P E i64)) (relation Signal (E)) (ruleset s)
(rule ((Signal x) (P x n) (< n 10)) ((P x (+ n 1))) :ruleset s)
(rule ((P x n)) ((Signal x)) :ruleset s)
(P (Root 0) 0) (run-schedule (saturate s)) (check (P (Root 0) 10))''')
        self.assertGreater(b['local_saturation']['stats']['cycle_deferrals'],0)
        self.assertLessEqual(b['local_saturation']['stats']['max_chain'],3)
    def test_constructor_set_is_a_congruence_operation(self):
        _,(a,b)=self.pair('''(datatype E (N i64) (F E) (G E) (H E)) (ruleset s)
(rewrite (F x) (G x) :ruleset s)
(rule ((G x)) ((H x)) :ruleset s)
(F (N 0)) (run-schedule (saturate s)) (check (H (N 0)))''')
        self.assertGreater(b['local_saturation']['stats']['local_bindings'],0)
        self.assertLess(len(b['steps']),len(a['steps']))
    def test_hide_keeps_original_snapshot_behavior(self):
        _,(_,b)=self.pair(CHAIN+'''(rule ((Q x)) ((subsume (Q x))) :ruleset s)
(run-schedule (saturate s))''')
        self.assertEqual(b['local_saturation']['stats']['local_bindings'],0)
    def test_preexisting_hidden_constructor_is_not_a_new_fact(self):
        source=CHAIN.replace('(ruleset s)','(ruleset s) (ruleset hide)')+'''
(Q (N 0)) (rule () ((subsume (Q (N 0)))) :ruleset hide)
(run-schedule (seq (run hide) (saturate s)))'''
        _,(_,b)=self.pair(source)
        self.assertGreater(b['local_saturation']['stats']['incomplete_contexts'],0)
        self.assertFalse(any(r['table']=='R' for r in json.loads((self.root/'1/runtime/state.json').read_text())['rows']))
    def test_completion_does_not_escape_its_saturation(self):
        source=CHAIN.replace('(ruleset s)','(ruleset s) (ruleset change)')+'''
(rule () ((P (N 1))) :ruleset change)
(run-schedule (seq (saturate s) (run change) (run s)))'''
        self.pair(source)
        state=json.loads((self.root/'1/runtime/state.json').read_text())
        self.assertEqual(sum(r['table']=='R' for r in state['rows']),1)
    def test_checkpoint_keeps_completion_receipts(self):
        p,_=self.pair(CHAIN+'(run-schedule (saturate s))')
        env={**SETTINGS,'CELL_CPU_LOCAL_SAT':'1'}
        execute(p,self.root/'pause',ENGINE,checkpoint_after=1,environment=env)
        execute(p,self.root/'resume',ENGINE,resume=self.root/'pause/runtime/checkpoint.json',environment=env)
        self.assertEqual(json.loads((self.root/'resume/runtime/state.json').read_text()),json.loads((self.root/'1/runtime/state.json').read_text()))
        with self.assertRaises(subprocess.CalledProcessError):execute(p,self.root/'bad-resume',ENGINE,resume=self.root/'pause/runtime/checkpoint.json',environment={**env,'CELL_CPU_LOCAL_SAT':'0'})
    def test_existing_semantics(self):
        root=self.root
        try:
            for name,source in CASES.items():
                with self.subTest(case=name):self.root=root/name;self.root.mkdir();self.pair(source,native=False)
        finally:self.root=root
if __name__=='__main__':unittest.main()
