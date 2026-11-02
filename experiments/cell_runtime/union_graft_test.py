"""Event-driven Spine entry: exact stages/bindings plus native tiny cases."""
import json, os, subprocess, tempfile, unittest
from pathlib import Path
from eggfuse.engine.cell_runtime import prepare, execute
from eggfuse.ir.cell_runtime import build
from eggfuse.engine.cpu import compare_native
from eggfuse.ir.recipe import digest
from experiments.cell_runtime.audit import check as audit
from experiments.cell_runtime.reference_views_test import FRONTEND, ENGINE
from experiments.cell_runtime.check import CASES

BASE='''(datatype E (N i64) (Bop E E) (Holder E) (Link E E) (Upgraded E E) (Next E E))
(ruleset alias) (ruleset consume) (ruleset next) (ruleset late) (ruleset hide)
(rule ((Link a b)) ((union a b)) :ruleset alias)
(rule ((Holder x) (= x (Bop a b))) ((Upgraded a b)) :ruleset consume)
(rule ((Upgraded a b)) ((Next a b)) :ruleset next)
'''
FACTS='''(Holder (N 0)) (Link (N 0) (Bop (N 1) (N 2)))
'''

class UnionGraftTests(unittest.TestCase):
    def setUp(self):self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
    def tearDown(self):self.tmp.cleanup()
    def pair(self,source,extra=None,native=True):
        src=self.root/'source.egg';src.write_text(source);core=prepare(src,FRONTEND);p=build(core)
        reports=[]
        for m in ('0','1'):
            reports.append(execute(p,self.root/m,ENGINE,audit=True,save_checkpoint=True,environment={
                'CELL_CPU_UNION_GRAFT':m,'CELL_CPU_UNION_TRACE':'0','CELL_CPU_MARKER_REFUTATION':'0','CELL_CPU_SIEVE':'0',
                'CELL_CPU_CONGRUENCE':'0','CELL_CPU_REFERENCE_VIEWS':'1',**(extra or {})}))
        for name in ['state.json','stages.json','binding-audit.json']:
            self.assertEqual(*[json.loads((self.root/m/'runtime'/name).read_text()) for m in ('0','1')],name)
        self.assertEqual(audit(p,self.root/'0/runtime'),audit(p,self.root/'1/runtime'))
        if native:
            # Existing native frontend; diagnostics only, not timing.
            subprocess.run([str(FRONTEND),str(src),'--native-full','--export-to',str(self.root/'native')],capture_output=True,check=True)
            compare_native(core,json.loads((self.root/'1/runtime/state.json').read_text()),json.loads((self.root/'native/native-state.json').read_text()))
        return p,reports
    def test_nonroot_input_refinement_enters_spine_without_marker(self):
        p,(a,b)=self.pair(BASE+FACTS+'''(run-schedule (seq (run consume) (run alias) (run next) (run consume) (run next)))
(check (Next (N 1) (N 2)))''')
        self.assertTrue(p['multi_stage']['union_entries'])
        self.assertGreater(b['union_graft']['stats']['bindings'],0)
        self.assertLess(b['multi_stage']['stats']['inverse_reads'],a['multi_stage']['stats']['inverse_reads'])
        stages=json.loads((self.root/'1/runtime/stages.json').read_text())
        self.assertFalse(any(r['table']=='Next' for r in stages[2]['rows']))
        self.assertTrue(any(e['depth']==0 and 'Apply' in e['cause'] for e in b['union_lineage']['events']))
    def test_congruence_depth_is_causal_not_iteration_count(self):
        source='''(datatype E (N i64) (F E) (G E) (Link E E))
(ruleset alias) (rule ((Link a b)) ((union a b)) :ruleset alias)
(G (F (N 1))) (G (F (N 2))) (Link (N 1) (N 2))
(run-schedule (run alias)) (check (= (G (F (N 1))) (G (F (N 2)))))'''
        _,(_,b)=self.pair(source)
        events=b['union_lineage']['events'];self.assertEqual(sorted(e['depth'] for e in events),[0,1,2])
        self.assertEqual(len({e['epoch'] for e in events}),1)
        self.assertEqual(len(events),b['stats']['unions'])
    def test_multiple_causes_preserved(self):
        source='''(datatype E (N i64) (F E E) (Link E E)) (ruleset alias)
(rule ((Link a b)) ((union a b)) :ruleset alias)
(F (N 1) (N 3)) (F (N 2) (N 4)) (Link (N 1) (N 2)) (Link (N 3) (N 4))
(run-schedule (run alias))'''
        _,(_,b)=self.pair(source)
        cs=[e for e in b['union_lineage']['events'] if 'Congruence' in e['cause']]
        self.assertTrue(any(len(e['cause']['Congruence']['parents'])==2 and e['depth']==1 for e in cs))
    def test_transitive_union_keeps_canonicalization_causes(self):
        source='''(datatype E (N i64) (F E) (Link E E)) (ruleset alias)
(rule ((Link a b)) ((union a b)) :ruleset alias)
(F (N 1)) (F (N 3)) (Link (N 1) (N 2)) (Link (N 2) (N 3))
(run-schedule (run alias)) (check (= (F (N 1)) (F (N 3))))'''
        _,(_,b)=self.pair(source);events=b['union_lineage']['events']
        self.assertTrue(any(e['canonicalization_parents'] for e in events))
        def applies(i):
            e=events[i];ids=set(e['canonicalization_parents']);result=set()
            if 'Apply' in e['cause']:result.add(e['cause']['Apply']['frame'])
            if 'Congruence' in e['cause']:ids.update(e['cause']['Congruence']['parents'])
            for j in ids:result.update(applies(j))
            return result
        self.assertTrue(any('Congruence' in e['cause'] and len(applies(i))==2 for i,e in enumerate(events)))
    def test_alias_alternatives_all_match(self):
        source=BASE+'''(rule () ((Link (N 0) (Bop (N 3) (N 4)))) :ruleset late)
'''+FACTS+'''(run-schedule (seq (run alias) (run consume) (run late) (run alias) (run consume)))
(check (Upgraded (N 1) (N 2)) (Upgraded (N 3) (N 4)))'''
        _,(_,b)=self.pair(source);self.assertGreaterEqual(b['union_graft']['stats']['bindings'],2)
    def test_subsume_invalidates_neighborhood(self):
        source=BASE+'''(rule ((= x (Bop a b))) ((subsume (Bop a b))) :ruleset hide)
'''+FACTS+'(run-schedule (seq (run alias) (run hide) (run consume)))'
        _,(_,b)=self.pair(source);self.assertEqual(b['union_graft']['stats']['bindings'],0)
        self.assertGreater(b['union_graft']['stats']['expired_table_domains'],0)
    def test_no_union_keeps_ordinary_matcher(self):
        _,(_,b)=self.pair(BASE+'(Holder (Bop (N 1) (N 2))) (run-schedule (run consume))')
        self.assertEqual(b['union_graft']['stats']['bindings'],0)
    def test_guards_and_repeated_binding(self):
        source=BASE.replace('(Holder x) (= x (Bop a b))','(Holder x) (= x (Bop a a))').replace('((Upgraded a b)) :ruleset consume','((Upgraded a a)) :ruleset consume')+FACTS+'(run-schedule (seq (run alias) (run consume)))'
        _,(_,b)=self.pair(source);self.assertEqual(b['union_graft']['stats']['bindings'],0)
    def test_numeric_guard_after_alias_alternatives(self):
        source='''(datatype E (Seed i64) (Num i64) (Holder E) (Link E E) (Good i64))
(ruleset alias) (ruleset consume) (ruleset late)
(rule ((Link a b)) ((union a b)) :ruleset alias)
(rule ((Holder x) (= x (Num n)) (> n 0)) ((Good n)) :ruleset consume)
(rule () ((Link (Seed 0) (Num 2))) :ruleset late)
(Holder (Seed 0)) (Link (Seed 0) (Num -1))
(run-schedule (seq (run alias) (run consume) (run late) (run alias) (run consume)))
(check (Good 2))'''
        _,(_,b)=self.pair(source);self.assertEqual(b['union_graft']['stats']['bindings'],1)
    def test_ordinary_join_is_replaced_when_no_existing_point_shortcut(self):
        _,(a,b)=self.pair(BASE+FACTS+'(run-schedule (seq (run alias) (run consume)))',{'CELL_CPU_ENTRY_PROGRAMS':'0'})
        self.assertGreater(b['union_graft']['stats']['bindings'],0)
        self.assertLess(b['stats']['residual_candidates'],a['stats']['residual_candidates'])
    def test_checkpoint_retains_causes_but_rebuilds_query_state(self):
        p,(_,b)=self.pair(BASE+FACTS+'(run-schedule (seq (run alias) (run consume) (run next)))')
        env={'CELL_CPU_UNION_GRAFT':'1','CELL_CPU_CONGRUENCE':'0','CELL_CPU_REFERENCE_VIEWS':'1'}
        execute(p,self.root/'partial',ENGINE,checkpoint_after=1,environment=env)
        execute(p,self.root/'resume',ENGINE,resume=self.root/'partial/runtime/checkpoint.json',environment=env)
        self.assertEqual(json.loads((self.root/'resume/runtime/state.json').read_text()),json.loads((self.root/'1/runtime/state.json').read_text()))
    def test_forged_entry_is_rejected_by_rust(self):
        src=self.root/'source.egg';src.write_text(BASE+FACTS+'(run-schedule (seq (run alias) (run consume)))');p=build(prepare(src,FRONTEND))
        self.assertTrue(p['multi_stage']['union_entries']);p['multi_stage']['union_entries'][0]['columns'][0]+=1
        p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'});path=self.root/'forged.json';path.write_text(json.dumps(p))
        r=subprocess.run([str(ENGINE),'--program',str(path),'--inspect-program'],capture_output=True,text=True)
        self.assertNotEqual(r.returncode,0);self.assertIn('union Spine entry',r.stderr)
    def test_legacy_artifact_cannot_silently_enable_graft(self):
        src=self.root/'source.egg';src.write_text(BASE+FACTS+'(run-schedule (seq (run alias) (run consume)))');p=build(prepare(src,FRONTEND))
        p['multi_stage'].pop('union_entries');p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
        path=self.root/'legacy.json';path.write_text(json.dumps(p))
        r=subprocess.run([str(ENGINE),'--program',str(path),'--inspect-program'],capture_output=True,text=True,env={**os.environ,'CELL_CPU_UNION_GRAFT':'1'})
        self.assertNotEqual(r.returncode,0);self.assertIn('re-emit',r.stderr)
    def test_existing_semantic_cases_keep_exact_stages_and_bindings(self):
        original=self.root
        try:
            for name,source in CASES.items():
                with self.subTest(case=name):
                    self.root=original/name;self.root.mkdir()
                    self.pair(source,native=False)
        finally:self.root=original

if __name__=='__main__':unittest.main()
