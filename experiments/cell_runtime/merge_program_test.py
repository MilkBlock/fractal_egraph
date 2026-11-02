"""Parameterized merge execution, original-stage visibility and full effects."""
import copy,json,os,subprocess,tempfile,unittest
from pathlib import Path
from eggfuse.engine.cell_runtime import execute,prepare
from eggfuse.ir.cell_runtime import build,validate
from eggfuse.ir.recipe import digest
from eggfuse.engine.cpu import compare_native
from experiments.cell_runtime.reference_views_test import FRONTEND,ENGINE
from experiments.cell_runtime.check import CASES

DECL='''(datatype E (N i64)) (datatype T (Term i64) (NoTerm))
(datatype C (Pair T i64) (Choose C C))
(function Best (E) C :merge (Choose old new))
(relation Proposal (E C)) (ruleset update) (ruleset resolve) (ruleset late) (ruleset alias) (ruleset hide)
(rule ((Proposal e (Pair t cost)) (= (Best e) (Pair oldt oldcost)) (< cost oldcost))
 ((set (Best e) (Pair t cost))) :ruleset update)
(rule ((= s (Choose (Pair ta ca) (Pair tb cb))) (< ca cb)) ((union s (Pair ta ca))) :ruleset resolve)
(rule ((= s (Choose (Pair ta ca) (Pair tb cb))) (> ca cb)) ((union s (Pair tb cb))) :ruleset resolve)
(rule ((= s (Choose (Pair ta ca) (Pair tb cb))) (= ca cb)) ((union s (Pair ta ca))) :ruleset resolve)
'''

class MergeProgramTests(unittest.TestCase):
    def setUp(self):self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
    def tearDown(self):self.temp.cleanup()
    def pair(self,source,native=True):
        src=self.root/'source.egg';src.write_text(source);core=prepare(src,FRONTEND);p=build(core);validate(p)
        rr=[]
        for flag in ('0','1'):
            rr.append(execute(p,self.root/flag,ENGINE,audit=True,save_checkpoint=True,environment={
                'CELL_CPU_MERGE_PROGRAMS':flag,'CELL_CPU_UNION_GRAFT':'0','CELL_CPU_UNION_IMPACT':'0',
                'CELL_CPU_UNION_TRACE':'0','CELL_CPU_MARKER_REFUTATION':'0','CELL_CPU_CONGRUENCE':'0','CELL_CPU_SIEVE':'0'}))
        for name in ['state.json','stages.json','binding-audit.json']:
            self.assertEqual(*[json.loads((self.root/f/'runtime'/name).read_text()) for f in ('0','1')],name)
        if native:
            subprocess.run([str(FRONTEND),str(src),'--native-full','--export-to',str(self.root/'native')],capture_output=True,check=True)
            compare_native(core,json.loads((self.root/'1/runtime/state.json').read_text()),json.loads((self.root/'native/native-state.json').read_text()))
        return p,rr
    def test_conflicts_keep_intermediate_nodes_and_stage_order(self):
        source=DECL+'''(rule ((Proposal e c)) ((set (Best e) (Pair (NoTerm) 10000000000000000))) :ruleset update)
(set (Best (N 0)) (Pair (Term 0) 10)) (Proposal (N 0) (Pair (Term 1) 7))
(run-schedule (seq (run update) (run resolve) (saturate resolve)))
(check (= (Best (N 0)) (Pair (Term 1) 7)))'''
        p,(_,r)=self.pair(source)
        self.assertGreater(r['stats']['merge_program_calls'],0)
        self.assertGreater(r['stats']['merge_program_constructs'],0)
        self.assertGreater(r['multi_stage']['stats']['merge_bindings'],0)
        stages=json.loads((self.root/'1/runtime/stages.json').read_text())
        self.assertGreater(sum(row['table']=='Choose' for row in stages[1]['rows']),1)
        self.assertEqual(len(p['merge_programs']['entries']),3)
    def test_late_candidates_and_alias_multiple_costs(self):
        source=DECL+'''(rule () ((union (Pair (Term 0) 10) (Pair (Term 2) 3))) :ruleset alias)
(rule () ((Proposal (N 0) (Pair (Term 3) 2))) :ruleset late)
(set (Best (N 0)) (Pair (Term 0) 10)) (Proposal (N 0) (Pair (Term 1) 7))
(run-schedule (seq (run update) (run alias) (saturate resolve) (run late) (run update) (saturate resolve)))'''
        _,(_,r)=self.pair(source)
        self.assertGreater(r['multi_stage']['stats']['merge_bindings'],0)
    def test_hidden_candidate_not_consumed(self):
        self.pair(DECL+'''(rule ((= p (Pair (Term 0) 10))) ((subsume (Pair (Term 0) 10))) :ruleset hide)
(set (Best (N 0)) (Pair (Term 0) 10)) (Proposal (N 0) (Pair (Term 1) 7))
(run-schedule (seq (run update) (run hide) (run resolve)))''')
    def test_tie_keeps_source_left_rule(self):
        self.pair(DECL+'''(Choose (Pair (Term 0) 5) (Pair (Term 1) 5))
(run-schedule (saturate resolve))
(check (= (Choose (Pair (Term 0) 5) (Pair (Term 1) 5)) (Pair (Term 0) 5)))''')
    def test_exact_i64_and_unseen_values(self):
        root=self.root;plans=[]
        try:
            for k,(a,b) in enumerate([(10000000000000001,10000000000000000),(-1,-2)]):
                self.root=root/str(k);self.root.mkdir()
                p,_=self.pair(DECL+f'''(set (Best (N 0)) (Pair (Term 0) {a})) (Proposal (N 0) (Pair (Term 1) {b}))
(run-schedule (seq (run update) (saturate resolve))) (check (= (Best (N 0)) (Pair (Term 1) {b})))''')
                # The check embeds the expected concrete number in source IR;
                # executable merge instructions and consumer wiring are shared.
                plans.append({k:p['merge_programs'][k] for k in ['programs','entries','unsupported','contract']})
            self.assertEqual(*plans)
        finally:self.root=root
    def test_checkpoint_needs_no_online_compiler(self):
        p,(_,r)=self.pair(DECL+'''(set (Best (N 0)) (Pair (Term 0) 10)) (Proposal (N 0) (Pair (Term 1) 7))
(run-schedule (seq (run update) (saturate resolve)))''')
        env={'CELL_CPU_MERGE_PROGRAMS':'1'}
        execute(p,self.root/'paused',ENGINE,checkpoint_after=1,environment=env)
        execute(p,self.root/'resumed',ENGINE,resume=self.root/'paused/runtime/checkpoint.json',environment=env)
        self.assertEqual(json.loads((self.root/'resumed/runtime/state.json').read_text()),json.loads((self.root/'1/runtime/state.json').read_text()))
    def test_nested_merge_reads_and_arithmetic(self):
        _,(_,r)=self.pair('''(datatype E (N i64)) (function Cost (E) i64 :merge (min old new))
(function F (i64) i64 :merge (+ (Cost (N 0)) (+ old new)))
(set (Cost (N 0)) 4) (set (F 0) 2)
(ruleset a) (rule () ((set (F 0) 3)) :ruleset a)
(run-schedule (run a)) (check (= (F 0) 9))''')
        self.assertGreater(r['stats']['merge_port_reads'],0)
    def test_repeated_constructor_and_primitive_equal_inputs(self):
        self.pair('''(datatype E (N i64)) (function Sum (E) i64 :merge (+ old new))
(set (Sum (N 0)) 3) (ruleset a) (rule () ((set (Sum (N 0)) 3)) :ruleset a)
(run-schedule (run a)) (check (= (Sum (N 0)) 6))''')
    def test_merge_failure_behavior_is_preserved(self):
        for label,source in {
            'overflow':'''(datatype E (N i64)) (function F (E) i64 :merge (+ old new))
(set (F (N 0)) 9223372036854775807) (ruleset a) (rule () ((set (F (N 0)) 1)) :ruleset a) (run-schedule (run a))''',
            'missing-port':'''(datatype E (N i64)) (function Score (E) i64 :merge (min old new))
(function F (E) i64 :merge (+ (Score (N 0)) (+ old new)))
(set (F (N 0)) 1) (ruleset a) (rule () ((set (F (N 0)) 2)) :ruleset a) (run-schedule (run a))'''
        }.items():
            with self.subTest(case=label):
                src=self.root/(label+'.egg');src.write_text(source);p=build(prepare(src,FRONTEND));logs=[]
                for flag in ('0','1'):
                    folder=self.root/(label+flag)
                    with self.assertRaises(subprocess.CalledProcessError):execute(p,folder,ENGINE,environment={'CELL_CPU_MERGE_PROGRAMS':flag})
                    logs.append((folder/'runtime.log').read_text())
                self.assertEqual(*logs)
    def test_forged_recipe_rejected_without_trusting_checksums(self):
        src=self.root/'source.egg';src.write_text(DECL+'(run-schedule (run resolve))');p=build(prepare(src,FRONTEND))
        for change in ['opcode','stage','entry']:
            q=copy.deepcopy(p)
            if change=='opcode':q['merge_programs']['programs'][0]['operations'][0]['args'].reverse()
            elif change=='stage':q['merge_programs']['control'].append({'run':'resolve'})
            else:q['merge_programs']['entries'][0]['atoms']=[]
            q['sha256']=digest({k:v for k,v in q.items() if k!='sha256'})
            f=self.root/(change+'.json');f.write_text(json.dumps(q))
            with self.assertRaises(ValueError):validate(q)
            r=subprocess.run([str(ENGINE),'--program',str(f),'--inspect-program'],capture_output=True,text=True)
            self.assertNotEqual(r.returncode,0);self.assertIn('merge execution recipe',r.stderr)
    def test_projection_marker_relays_merge_context(self):
        source=DECL.split('(rule ((= s (Choose')[0]+'''
(function Digest (C C C T i64) Unit :merge old) (ruleset publish)
(rule ((= s (Choose l r)) (= l (Pair ta ca))) ((set (Digest s l r ta ca) ())) :ruleset publish)
(rule ((Digest s l r ta ca) (= r (Pair tb cb)) (> ca cb)) ((union s r)) :ruleset resolve)
(set (Best (N 0)) (Pair (Term 0) 10)) (Proposal (N 0) (Pair (Term 1) 7))
(run-schedule (seq (run update) (run resolve) (run publish) (run resolve)))
(check (= (Best (N 0)) (Pair (Term 1) 7)))'''
        p,(_,r)=self.pair(source)
        self.assertEqual([x['table'] for x in p['merge_programs']['relays']],['Digest'])
        self.assertGreater(r['multi_stage']['stats']['merge_guard_checks'],0)
    def test_existing_artifact_cannot_silently_enable_program(self):
        src=self.root/'source.egg';src.write_text(DECL+'(run-schedule (run resolve))');p=build(prepare(src,FRONTEND))
        p.pop('merge_programs');p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'});validate(p)
        f=self.root/'old.json';f.write_text(json.dumps(p))
        r=subprocess.run([str(ENGINE),'--program',str(f),'--inspect-program'],capture_output=True,text=True,env={**os.environ,'CELL_CPU_MERGE_PROGRAMS':'1'})
        self.assertNotEqual(r.returncode,0);self.assertIn('re-emitting',r.stderr)
    def test_three_stage_candidate_program_and_later_child_update(self):
        source=DECL.replace('(datatype E (N i64))','(datatype E (N i64) (Add E E))').replace('(Term i64) (NoTerm)','(Term i64) (Sum T T) (NoTerm)')+'''
(ruleset produce) (ruleset improve)
(rule ((= e (Add x y)) (= (Best x) (Pair tx cx)) (= (Best y) (Pair ty cy)))
 ((Proposal e (Pair (Sum tx ty) (+ 1 (+ cx cy))))) :ruleset produce)
(rule () ((Proposal (N 0) (Pair (Term 2) 2))) :ruleset improve)
(set (Best (N 0)) (Pair (Term 0) 4)) (set (Best (N 1)) (Pair (Term 1) 6))
(set (Best (Add (N 0) (N 1))) (Pair (NoTerm) 10000000000000000))
(run-schedule (seq
 (saturate (seq (run produce) (saturate (seq (run update) (saturate resolve)))))
 (run improve)
 (saturate (seq (run produce) (saturate (seq (run update) (saturate resolve)))))))
(check (= (Best (Add (N 0) (N 1))) (Pair (Sum (Term 2) (Term 1)) 9)))'''
        self.pair(source)
    def test_unchanged_union_side_does_not_republish_default_candidate(self):
        self.pair(DECL+'''(rule ((Proposal e c)) ((set (Best e) (Pair (NoTerm) 10000000000000000))) :ruleset update)
(Proposal (N 0) (Pair (Term 0) 1))
(run-schedule (saturate (seq (run update) (saturate resolve))))
(check (= (Best (N 0)) (Pair (Term 0) 1)))''')
    def test_existing_semantic_cases(self):
        root=self.root
        try:
            for name,source in CASES.items():
                with self.subTest(case=name):
                    self.root=root/name;self.root.mkdir();self.pair(source,native=False)
        finally:self.root=root

if __name__=='__main__':unittest.main()
