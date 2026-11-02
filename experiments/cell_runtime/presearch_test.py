"""Positive residual contexts: complete bindings, delta ownership and invalidation."""
import json,subprocess,tempfile,unittest
from pathlib import Path
from eggfuse.engine.cell_runtime import prepare,execute
from eggfuse.ir.cell_runtime import build
from eggfuse.engine.cpu import compare_native
from experiments.cell_runtime.reference_views_test import FRONTEND,ENGINE
from experiments.cell_runtime.presearch_bench import SETTINGS
from experiments.cell_runtime.check import CASES

DECL='''(datatype E (N i64)) (relation Anchor (E i64))
(relation R (E E)) (relation S (E E)) (relation Out (i64 E)) (relation Later (i64 E))
(ruleset a) (ruleset b) (ruleset change)
(rule ((Anchor x tag) (R x y) (S y z)) ((Out tag z)) :ruleset a)
(rule ((Anchor x tag) (R x y) (S y z)) ((Later tag z)) :ruleset b)
(R (N 0) (N 1)) (R (N 0) (N 2)) (S (N 1) (N 3)) (S (N 2) (N 4))
(Anchor (N 0) 10) (Anchor (N 0) 11) (Anchor (N 0) 12)
'''

class PresearchTests(unittest.TestCase):
    def setUp(self):self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
    def tearDown(self):self.tmp.cleanup()
    def pair(self,source,extra=None,native=False):
        src=self.root/'source.egg';src.write_text(source);core=prepare(src,FRONTEND);program=build(core)
        reports=[]
        for flag in ('0','observe','1','marker'):
            reports.append(execute(program,self.root/flag,ENGINE,audit=True,
                environment={**SETTINGS,**(extra or {}),'CELL_CPU_SEARCH_CONTEXTS':flag}))
        for name in ['state.json','stages.json','binding-audit.json']:
            values=[json.loads((self.root/f/'runtime'/name).read_text()) for f in ('0','observe','1','marker')]

            for v in values[1:]:self.assertEqual(values[0],v,name)
        if native:
            subprocess.run([str(FRONTEND),str(src),'--native-full','--export-to',str(self.root/'native')],capture_output=True,check=True)
            compare_native(core,json.loads((self.root/'1/runtime/state.json').read_text()),json.loads((self.root/'native/native-state.json').read_text()))
        return program,reports
    def test_shared_inputs_across_roots_rules_and_stages(self):
        _,r=self.pair(DECL+'(run-schedule (seq (run a) (run b)))',native=True)
        s=r[2]['search_contexts']['stats']
        self.assertGreater(s['hits'],0);self.assertGreater(s['later_stage_hits'],0)
        self.assertGreater(s['other_rule_hits'],0)
        self.assertGreater(s['incomplete'],0)
        self.assertLess(r[2]['stats']['residual_candidates'],r[0]['stats']['residual_candidates'])
    def test_cached_complete_domain_rechecks_each_delta_pivot(self):
        source=DECL.replace('(relation Anchor (E i64))','(relation Anchor (E i64)) (relation Early (E i64))').replace('(R (N 0) (N 1))',
            '(Early (N 0) 19) (R (N 0) (N 1))',1)
        source=source.replace('(rule ((Anchor x tag) (R x y) (S y z)) ((Later tag z))',
            '(rule ((Early x tag) (R x y) (S y z)) ((Later tag z))')
        source=source.replace('(Anchor (N 0) 10) (Anchor (N 0) 11) (Anchor (N 0) 12)',
            '(ruleset seed) (rule () ((Anchor (N 0) 10) (Anchor (N 0) 11) (Anchor (N 0) 12)) :ruleset seed)')
        _,r=self.pair(source+'(run-schedule (seq (run seed) (run a) (run b)))',native=True)
        self.assertGreater(r[2]['search_contexts']['stats']['delta_rejected_answers'],0)
    def test_union_adds_candidates_and_preserves_correlated_binding(self):
        _,r=self.pair(DECL+'''(rule () ((union (N 1) (N 2)) (Anchor (N 0) 13)) :ruleset change)
(run-schedule (seq (run a) (run change) (run b) (run a)))''',native=True)
        self.assertGreater(r[2]['search_contexts']['stats']['invalidated'],0)
    def test_delete_and_hidden_witnesses_invalidate(self):
        root=self.root
        try:
            for op in ('delete','subsume'):
                self.root=root/op;self.root.mkdir()
                _,r=self.pair(DECL+f'''(rule () (({op} (R (N 0) (N 1)))) :ruleset change)
(run-schedule (seq (run a) (run change) (run b)))''',native=True)
                self.assertGreater(r[2]['search_contexts']['stats']['invalidated'],0)
        finally:self.root=root
    def test_function_update_with_forced_fingerprint_collision(self):
        source=DECL+'''(function Cost (E) i64 :merge new) (relation Answer (i64 E i64))
(set (Cost (N 3)) 9) (set (Cost (N 4)) 7)
(ruleset calc)
(rule ((Anchor x tag) (R x y) (S y z) (= c (Cost z))) ((Answer tag z c)) :ruleset calc)
(rule () ((set (Cost (N 3)) 2) (Anchor (N 0) 14)) :ruleset change)
(run-schedule (seq (run calc) (run change) (run calc)))'''
        _,r=self.pair(source,{'CELL_CPU_HASH_BITS':'0'},native=True)
        self.assertGreater(r[2]['search_contexts']['stats']['invalidated'],0)
    def test_late_inputs_repeat_and_checkpoint(self):
        p,r=self.pair(DECL+'''(rule () ((S (N 1) (N 5)) (Anchor (N 0) 13)) :ruleset change)
(run-schedule (seq (repeat 2 (run a)) (run change) (run b) (saturate a)))''',native=True)
        execute(p,self.root/'paused',ENGINE,checkpoint_after=1,environment={**SETTINGS,'CELL_CPU_SEARCH_CONTEXTS':'1'})
        execute(p,self.root/'resumed',ENGINE,resume=self.root/'paused/runtime/checkpoint.json',environment={**SETTINGS,'CELL_CPU_SEARCH_CONTEXTS':'1'})
        self.assertEqual(json.loads((self.root/'resumed/runtime/state.json').read_text()),json.loads((self.root/'1/runtime/state.json').read_text()))
    def test_real_marker_merge_entry(self):
        from experiments.cell_runtime.merge_program_test import DECL as MERGE
        source=MERGE.split('(rule ((= s (Choose')[0]+'''(function Digest (C C C T i64) Unit :merge old) (ruleset publish)
(rule ((= s (Choose l r)) (= l (Pair ta ca))) ((set (Digest s l r ta ca) ())) :ruleset publish)
(rule ((Digest s l r ta ca) (= r (Pair tb cb)) (> ca cb)) ((union s r)) :ruleset resolve)
(set (Best (N 0)) (Pair (Term 0) 10)) (Proposal (N 0) (Pair (Term 1) 7))
(run-schedule (seq (run update) (run publish) (saturate resolve)))'''
        self.pair(source,native=True)
    def test_existing_semantics(self):
        root=self.root
        try:
            for name,source in CASES.items():
                with self.subTest(case=name):self.root=root/name;self.root.mkdir();self.pair(source)
        finally:self.root=root

if __name__=='__main__':unittest.main()
