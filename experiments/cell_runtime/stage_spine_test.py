"""One Spine runs A -> B -> C through actual RHS recipes at source stages."""
import json,unittest
from experiments.cell_runtime import retained_test as rt
from eggfuse.engine.cell_runtime import execute

CHAIN='''(datatype E (N i64) (Seed E) (P E) (Q E) (R E) (Known E))
(ruleset a) (ruleset b) (ruleset c)
(rule ((Seed x)) ((P x) (Known x)) :ruleset a)
(rule ((P x) (Known x)) ((Q x)) :ruleset b)
(rule ((Q x) (Known x)) ((R x)) :ruleset c)
(Seed (N 0))
'''
class StageSpineTests(unittest.TestCase):
    setUp=rt.RetainedTests.setUp
    tearDown=rt.RetainedTests.tearDown
    def pair(self,source,native=True):
        from eggfuse.engine.cell_runtime import prepare
        from eggfuse.ir.cell_runtime import build
        from eggfuse.engine.cpu import compare_native
        import subprocess
        src=self.root/'source.egg';src.write_text(source);core=prepare(src,rt.FRONTEND);p=build(core);reports=[]
        for flag in ('0','1'):
            reports.append(execute(p,self.root/flag,rt.ENGINE,audit=True,environment={**rt.SETTINGS,'CELL_CPU_STAGE_SPINE':flag,'CELL_CPU_LOCAL_SAT':'0','CELL_CPU_RETAINED_CONTEXTS':'0'}))
        for file in ('state.json','stages.json','binding-audit.json'):
            self.assertEqual(json.loads((self.root/'0/runtime'/file).read_text()),json.loads((self.root/'1/runtime'/file).read_text()),file)
        if native:
            subprocess.run([str(rt.FRONTEND),str(src),'--native-full','--export-to',str(self.root/'native')],check=True,capture_output=True)
            compare_native(core,json.loads((self.root/'1/runtime/state.json').read_text()),json.loads((self.root/'native/native-state.json').read_text()))
        return p,reports
    def test_one_spine_completes_three_stages(self):
        _,(_,r)=self.pair(CHAIN+'(run-schedule (seq (run a) (run b) (run c))) (check (R (N 0)))')
        s=r['stage_spine']['stats'];self.assertEqual(s['resumed_steps'],2)
        self.assertEqual(s['max_chain'],3);self.assertEqual(s['max_sets_per_spine'],3)
        self.assertGreaterEqual(s['query_takeovers'],2)
    def test_later_stage_uses_ancestor_fact_not_read_by_middle_stage(self):
        source=CHAIN.replace('(rule ((P x) (Known x)) ((Q x))', '(rule ((P x)) ((Q x))')
        _,(_,r)=self.pair(source+'(run-schedule (seq (run a) (run b) (run c)))')
        s=r['stage_spine']['stats']
        self.assertEqual(s['resumed_steps'],2);self.assertEqual(s['max_chain'],3)
        self.assertGreater(s['context_reads'],0)
        self.assertEqual(s['port_refreshes'],0)
    def test_finite_repeat_respects_each_stage(self):
        self.pair(CHAIN+'(run-schedule (repeat 2 (seq (run c) (run b) (run a))))')
    def test_sibling_rules_cannot_observe_future_effects(self):
        self.pair(CHAIN.replace(':ruleset b',':ruleset a').replace(':ruleset c',':ruleset a')+'(run-schedule (run a))')
    def test_union_and_function_updates_refresh_real_parameters(self):
        self.pair('''(datatype E (N i64) (Seed E) (P E) (R E i64))
(function Cost (E) i64 :merge new) (ruleset a) (ruleset b) (ruleset update)
(rule ((Seed x) (= n (Cost x))) ((P x)) :ruleset a)
(rule ((P x) (= n (Cost x))) ((R x n)) :ruleset b)
(rule () ((set (Cost (N 1)) 7) (union (N 0) (N 1))) :ruleset update)
(Seed (N 1)) (set (Cost (N 1)) 3) (N 0)
(run-schedule (seq (run a) (run update) (run b)))''')
    def test_custom_merge_effects_and_numeric_guards(self):
        self.pair('''(datatype T (Pair i64) (Choose T T)) (datatype E (N i64) (Seed E) (P E) (Known E))
(function Best (E) T :merge (Choose old new))
(ruleset a) (ruleset b)
(rule ((Seed x)) ((P x) (Known x)) :ruleset a)
(rule ((P x) (Known x)) ((set (Best x) (Pair 2)) (set (Best x) (Pair 1))) :ruleset b)
(Seed (N 0)) (run-schedule (seq (run a) (run b)))''')
    def test_unknown_contexts_still_use_ordinary_matching(self):
        _,(_,r)=self.pair(CHAIN+'(P (N 1)) (Known (N 1)) (run-schedule (seq (run a) (run b) (run c))) (check (R (N 1)))')
        self.assertGreater(r['stage_spine']['stats']['uncovered_queries'],0)
    def test_hidden_evidence_does_not_fire_later_stage(self):
        self.pair(CHAIN+'''(ruleset hide) (rule () ((subsume (Known (N 0)))) :ruleset hide)
(run-schedule (seq (run a) (run hide) (run b) (run c)))''')
    def test_parameter_update_can_change_guard(self):
        self.pair('''(datatype E (N i64) (Seed E) (P E) (Out E i64))
(function Cost (E) i64 :merge new) (ruleset a) (ruleset b) (ruleset change)
(rule ((Seed x) (= n (Cost x))) ((P x)) :ruleset a)
(rule ((P x) (= n (Cost x)) (> n 0)) ((Out x n)) :ruleset b)
(rule () ((set (Cost (N 0)) -1)) :ruleset change)
(Seed (N 0)) (set (Cost (N 0)) 1)
(run-schedule (seq (run a) (run change) (run b)))''')
    def test_inverse_multi_candidate_port_remains_exhaustive(self):
        self.pair('''(datatype E (N i64) (Seed E) (P E) (F E) (Out E E))
(ruleset a) (ruleset b) (ruleset change)
(rule ((Seed x)) ((P x)) :ruleset a)
(rule ((P x) (= x (F y))) ((Out x y)) :ruleset b)
(rule () ((union (F (N 1)) (F (N 2)))) :ruleset change)
(Seed (F (N 1))) (F (N 2))
(run-schedule (seq (run a) (run change) (run b)))
(check (Out (F (N 1)) (N 1)) (Out (F (N 1)) (N 2)))''')
    def test_hidden_anchor_reinsertion_before_consumer(self):
        _,(_,r)=self.pair(CHAIN+'''(ruleset hide) (ruleset restore)
(rule () ((subsume (P (N 0)))) :ruleset hide)
(rule () ((P (N 0))) :ruleset restore)
(run-schedule (seq (run a) (run hide) (run restore) (run b) (run c)))''')
        self.assertGreater(r['stage_spine']['stats']['anchors_retired'],0)
    def test_artifact_plan_tampering_is_rejected(self):
        from eggfuse.ir.cell_runtime import build,validate
        from eggfuse.engine.cell_runtime import prepare
        src=self.root/'source.egg';src.write_text(CHAIN+'(run-schedule (run a))')
        p=build(prepare(src,rt.FRONTEND))
        p['multi_stage']['stage_entries'][0]['atoms']=[999]
        from eggfuse.ir.recipe import digest
        p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
        with self.assertRaises(ValueError):validate(p)
        path=self.root/'tampered.json';path.write_text(json.dumps(p))
        import subprocess
        proc=subprocess.run([str(rt.ENGINE),'--program',str(path),'--output',str(self.root/'bad')],capture_output=True,text=True)
        self.assertNotEqual(proc.returncode,0)
        self.assertIn('stage entry program differs',proc.stderr)
    def test_legacy_artifact_requires_recompile_only_when_enabled(self):
        from eggfuse.ir.cell_runtime import build,validate
        from eggfuse.ir.recipe import digest
        from eggfuse.engine.cell_runtime import prepare
        src=self.root/'source.egg';src.write_text(CHAIN+'(run-schedule (run a))')
        p=build(prepare(src,rt.FRONTEND));del p['multi_stage']['stage_entries']
        p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'});validate(p)
        execute(p,self.root/'legacy',rt.ENGINE,environment={**rt.SETTINGS,'CELL_CPU_STAGE_SPINE':'0'})
        import subprocess
        with self.assertRaises(subprocess.CalledProcessError):
            execute(p,self.root/'unsupported',rt.ENGINE,environment={**rt.SETTINGS,'CELL_CPU_STAGE_SPINE':'1'})
        self.assertIn('require compiler stage_entries',(self.root/'unsupported/runtime.log').read_text())
    def test_checkpoint_cold_programs_keep_semantics(self):
        p,_=self.pair(CHAIN+'(run-schedule (seq (run a) (run b) (run c)))')
        env={**rt.SETTINGS,'CELL_CPU_STAGE_SPINE':'1','CELL_CPU_LOCAL_SAT':'0'}
        execute(p,self.root/'pause',rt.ENGINE,checkpoint_after=1,environment=env)
        execute(p,self.root/'resume',rt.ENGINE,resume=self.root/'pause/runtime/checkpoint.json',environment=env)
        self.assertEqual(json.loads((self.root/'1/runtime/state.json').read_text()),json.loads((self.root/'resume/runtime/state.json').read_text()))
    def test_existing_semantics(self):
        root=self.root
        try:
            for name,source in rt.CASES.items():
                with self.subTest(case=name):self.root=root/name;self.root.mkdir();self.pair(source,native=False)
        finally:self.root=root
if __name__=='__main__':unittest.main()
