import copy,json,os,subprocess,tempfile,unittest
from pathlib import Path
from eggfuse.engine.cell_runtime import prepare,execute
from eggfuse.ir.cell_runtime import build,validate
from eggfuse.ir.recipe import digest
from experiments.cell_runtime.reference_views_test import FRONTEND,ENGINE
from experiments.cell_runtime.check import CASES
class KnownResultsTests(unittest.TestCase):
    def setUp(self):self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
    def tearDown(self):self.tmp.cleanup()
    def pair(self,source,duplicate_constructor=False):
        f=self.root/'source.egg';f.write_text(source);core=prepare(f,FRONTEND)
        if duplicate_constructor:
            r=next(r for s in core['rulesets'] for r in s['rules'] if any(e.get('call',{}).get('table')=='F' for e in r['head']))
            e=copy.deepcopy(next(e for e in r['head'] if e.get('call',{}).get('table')=='F'))
            e['dst']+='-marker-repeat';r['head'].append(e)
        p=build(core);reports=[]
        for mode in ('off','observe','on','value','static'):
            reports.append(execute(p,self.root/mode,ENGINE,audit=True,save_checkpoint=True,environment={'CELL_CPU_KNOWN_RESULTS':mode,'CELL_CPU_MERGE_PROGRAMS':'1'}))
        for name in ['state.json','stages.json','binding-audit.json']:
            data=[json.loads((self.root/m/'runtime'/name).read_text()) for m in ('off','observe','on','value','static')]
            for value in data[1:]:self.assertEqual(data[0],value)
        self.assertEqual(reports[1]['known_results']['stats']['observed_other_witness'],0)
        self.assertEqual(reports[1]['known_results']['stats']['observed_other_value'],0)
        return p,reports
    def test_lhs_and_prior_step_results(self):
        p,rr=self.pair('''(datatype E (N i64) (F E) (G E E)) (function Mark (E) Unit :merge old) (ruleset r)
(rule ((= x (N n))) ((set (Mark (F (N n))) ()) (G (F (N n)) (F (N n)))) :ruleset r)
(N 1) (run-schedule (run r)) (check (G (F (N 1)) (F (N 1))))''',duplicate_constructor=True)
        self.assertGreater(rr[2]['known_results']['stats']['avoided_lookups'],0)
        self.assertGreater(rr[2]['known_results']['stats']['effect_sources'],0)
        self.assertGreater(rr[2]['known_results']['stats']['query_sources'],0)
        self.assertGreater(rr[3]['known_results']['stats']['value_reuses'],0)
        self.assertGreater(rr[4]['known_results']['stats']['static_eliminated'],0)
    def test_deleted_source_must_reconstruct(self):
        _,rr=self.pair('''(datatype E (N i64) (F E)) (ruleset r)
(rule ((= x (N n))) ((delete (N n)) (F (N n))) :ruleset r)
(N 1) (run-schedule (run r)) (check (F (N 1)))''')
        self.assertGreater(rr[2]['known_results']['stats']['expired'],0)
        self.assertGreater(rr[3]['known_results']['stats']['deletion_guard_misses'],0)
    def test_subsumed_fact_keeps_original_construction_semantics(self):
        self.pair('''(datatype E (N i64) (F E)) (ruleset r)
(rule ((= x (N n))) ((subsume (N n)) (F (N n))) :ruleset r)
(N 1) (run-schedule (run r))''')
    def test_other_rule_delete_and_late_function_update(self):
        self.pair('''(datatype E (N i64) (F E) (V i64)) (function Cost (E) i64 :merge new) (ruleset r) (ruleset later)
(rule ((N n)) ((delete (N n))) :ruleset r)
(rule ((= x (N n)) (= a (Cost x))) ((F (N n)) (set (Cost x) 2) (V a)) :ruleset r)
(rule ((= a (Cost x))) ((V a)) :ruleset later)
(N 1) (set (Cost (N 1)) 1) (run-schedule (seq (run r) (run later))) (check (F (N 1)) (V 1) (V 2))''')
    def test_union_stage_and_checkpoint(self):
        p,rr=self.pair('''(datatype E (N i64) (F E) (G E E)) (ruleset alias) (ruleset r)
(rule ((N n)) ((union (N 1) (N 2))) :ruleset alias)
(rule ((= x (N n))) ((G (F (N n)) (F (N n)))) :ruleset r)
(N 1) (N 2) (run-schedule (seq (run alias) (run r)))''')
        execute(p,self.root/'pause',ENGINE,checkpoint_after=1,environment={'CELL_CPU_KNOWN_RESULTS':'on'})
        execute(p,self.root/'resume',ENGINE,resume=self.root/'pause/runtime/checkpoint.json',environment={'CELL_CPU_KNOWN_RESULTS':'on'})
        self.assertEqual(json.loads((self.root/'resume/runtime/state.json').read_text()),json.loads((self.root/'on/runtime/state.json').read_text()))
    def test_default_merge_program_is_enabled(self):
        f=self.root/'default.egg';f.write_text('''(datatype E (N i64)) (function F (E) i64 :merge (+ old new))
(set (F (N 0)) 2) (ruleset r) (rule () ((set (F (N 0)) 3)) :ruleset r) (run-schedule (run r))''')
        p=build(prepare(f,FRONTEND));r=execute(p,self.root/'default',ENGINE)
        self.assertTrue(r['features']['merge_programs']);self.assertGreater(r['stats']['merge_program_calls'],0)
    def test_static_checkpoint_requires_same_representation(self):
        p,_=self.pair('''(datatype E (N i64) (F E)) (ruleset r) (ruleset s)
(rule ((N n)) ((F (N n))) :ruleset r) (rule ((F x)) ((F x)) :ruleset s)
(N 1) (run-schedule (seq (run r) (run s)))''')
        execute(p,self.root/'static-pause',ENGINE,checkpoint_after=1,environment={'CELL_CPU_KNOWN_RESULTS':'static'})
        execute(p,self.root/'static-resume',ENGINE,resume=self.root/'static-pause/runtime/checkpoint.json',environment={'CELL_CPU_KNOWN_RESULTS':'static'})
        self.assertEqual(json.loads((self.root/'static-resume/runtime/state.json').read_text()),json.loads((self.root/'static/runtime/state.json').read_text()))
        with self.assertRaises(subprocess.CalledProcessError):execute(p,self.root/'bad-resume',ENGINE,resume=self.root/'static-pause/runtime/checkpoint.json',environment={'CELL_CPU_KNOWN_RESULTS':'off'})
    def test_forged_projection_rejected(self):
        f=self.root/'forged.egg';f.write_text('''(datatype E (N i64) (F E)) (ruleset r)
(rule ((N n)) ((F (N n))) :ruleset r) (N 1) (run-schedule (run r))''');p=build(prepare(f,FRONTEND))
        self.assertTrue(p['known_results']['plans']);p['known_results']['plans'][0]['source_index']+=1
        p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'});f=self.root/'forged.json';f.write_text(json.dumps(p))
        with self.assertRaises(ValueError):validate(p)
        r=subprocess.run([str(ENGINE),'--program',str(f),'--inspect-program'],capture_output=True,text=True)
        self.assertNotEqual(r.returncode,0);self.assertIn('known result',r.stderr)
    def test_existing_cases(self):
        root=self.root
        try:
            for name,source in CASES.items():
                with self.subTest(case=name):self.root=root/name;self.root.mkdir();self.pair(source)
        finally:self.root=root
if __name__=='__main__':unittest.main()
