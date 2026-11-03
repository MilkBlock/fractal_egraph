import copy,json,os,subprocess,tempfile,unittest
from pathlib import Path
from eggfuse.engine.cell_runtime import prepare,execute
from eggfuse.ir.cell_runtime import build,validate
from eggfuse.ir.recipe import digest
from eggfuse.analyzer.compositions import build as compose
from experiments.cell_runtime.reference_views_test import FRONTEND,ENGINE,norm
from experiments.cell_runtime.audit import check as audit
from experiments.cell_runtime.check import CASES

DECL='''(datatype E (N i64) (Get E i64) (Single E) (SubTuple E i64 i64) (Seen E))
(function Entry (E E i64) Unit :merge old)
(ruleset recognize) (ruleset first) (ruleset second) (ruleset third) (ruleset alias) (ruleset late)
(rule ((= s (SubTuple e i 1))) ((set (Entry s e i) ())) :ruleset recognize)
(rule ((Entry s e i)) ((union s (Single (Get e i)))) :ruleset first)
'''
SECOND='(rule ((= v (Single x))) ((union (Get (Single x) 0) x)) :ruleset second)\n'
THIRD='(rule ((= x (Get v 0))) ((Seen x)) :ruleset third)\n'
INITIAL='(SubTuple (N 1) 2 1) (SubTuple (N 2) 3 1)\n'
CONTROL='(run-schedule (seq (run recognize) (run first) (run second) (run third)))'
def read(path):return json.loads(path.read_text())

class CompositionTests(unittest.TestCase):
 def setUp(self):self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
 def tearDown(self):self.tmp.cleanup()
 def program(self,text,pairs=(('first','second'),('second','third'))):
  path=self.root/'source.egg';path.write_text(text);core=prepare(path,FRONTEND);p=build(core,reference_tables=['Entry'])
  by_set={r['metadata']['source_set']:i for i,r in enumerate(p['recipe']['rules'])}
  evidence={'recipe_sha256':p['recipe']['sha256'],'pairs':[[by_set[a],by_set[b]] for a,b in pairs]}
  plan=compose(p['recipe'],['Entry'],evidence)
  p=build(core,reference_tables=['Entry'],compositions=plan);validate(p);return p
 def pair(self,p):
  reports=[];env={'CELL_CPU_REFERENCE_VIEWS':'1','CELL_CPU_MARKER_FIELDS':'copy'}
  for mode in ('0','1'):
   reports.append(execute(p,self.root/mode,ENGINE,audit=True,save_checkpoint=True,environment={**env,'CELL_CPU_COMPOSITIONS':mode}))
  states=[read(self.root/m/'runtime/stages.json') for m in ('0','1')]
  self.assertEqual(list(map(norm,states[0])),list(map(norm,states[1])))
  self.assertEqual(read(self.root/'0/runtime/binding-audit.json'),read(self.root/'1/runtime/binding-audit.json'))
  self.assertEqual(audit(p,self.root/'0/runtime'),audit(p,self.root/'1/runtime'))
  return reports
 def test_real_subtuple_single_get_three_rule_program(self):
  p=self.program(DECL+SECOND+THIRD+INITIAL+CONTROL);a,b=self.pair(p)
  stats=b['compositions']['stats'];self.assertGreaterEqual(stats['max_steps'],3)
  self.assertGreater(stats['marker_continued_steps'],0);self.assertGreater(stats['constructor_reuses'],0)
  self.assertLess(b['stats']['constructor_requests'],a['stats']['constructor_requests'])
  self.assertTrue(any(s['equalities'] for c in p['compositions']['programs'] for s in c['steps']))
  env={'CELL_CPU_REFERENCE_VIEWS':'1','CELL_CPU_COMPOSITIONS':'1'}
  execute(p,self.root/'partial',ENGINE,checkpoint_after=2,environment=env)
  execute(p,self.root/'resumed',ENGINE,resume=self.root/'partial/runtime/checkpoint.json',environment=env)
  self.assertEqual(norm(read(self.root/'resumed/runtime/state.json')),norm(read(self.root/'1/runtime/state.json')))
 def test_finite_run_does_not_execute_future_step(self):
  p=self.program(DECL+SECOND+THIRD+INITIAL+'(run-schedule (seq (run recognize) (run second) (run first)))',pairs=(('first','second'),))
  _,b=self.pair(p);self.assertEqual(b['compositions']['stats']['continued_steps'],0)
 def test_short_circuited_rhs_does_not_certify_future_facts(self):
  decl=DECL.replace('((union s (Single (Get e i))))','((let u (Get e i)) (delete (Seen e)) (Single u))')
  source=decl+'(rule ((= u (Get e i)) (= v (Single u))) ((Seen v)) :ruleset second)\n'+INITIAL+'(run-schedule (seq (run recognize) (run first) (run second)))'
  p=self.program(source,pairs=(('first','second'),));self.assertTrue(any(i['kind']=='known' for e in p['compositions']['edges'] for i in e['instructions']))
  self.pair(p)
 def test_late_function_port_and_numeric_guard(self):
  source=DECL+'''(function Limit (E) i64 :merge (min old new))
(rule ((= v (Single x)) (= n (Limit x)) (> n 0)) ((union (Get v 0) x)) :ruleset second)
(rule ((= x (Get e i))) ((set (Limit x) 7)) :ruleset late)
'''+THIRD+INITIAL+'(set (Limit (N 999)) 7) (run-schedule (seq (run recognize) (run first) (run second) (run late) (run second) (run third)))'
  _,b=self.pair(self.program(source));self.assertGreater(b['compositions']['stats']['port_reads'],0)
 def test_union_creates_new_alias_bindings(self):
  source=DECL+SECOND+THIRD+'''(rule ((= a (N 1)) (= b (N 2))) ((union a b)) :ruleset alias)
'''+INITIAL+'(run-schedule (seq (run recognize) (run first) (run second) (run alias) (run recognize) (run first) (run second) (run third)))'
  self.pair(self.program(source))
 def test_removed_source_is_not_a_permanent_proof(self):
  source=DECL+SECOND+THIRD+'(rule ((Single x)) ((subsume (Single x))) :ruleset late)\n'+INITIAL+CONTROL
  p=self.program(source);self.assertFalse(any(e['producer']==1 and e['consumer']==2 for e in p['compositions']['edges']));self.pair(p)
 def test_forged_effect_and_guard_rejected(self):
  p=self.program(DECL+SECOND+THIRD+INITIAL+CONTROL)
  p['compositions']['programs'][0]['steps'][0]['effects']=[]
  p['compositions']['sha256']=digest({k:v for k,v in p['compositions'].items() if k!='sha256'});p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
  with self.assertRaisesRegex(ValueError,'proof differs'):validate(p)
  path=self.root/'forged.json';path.write_text(json.dumps(p));r=subprocess.run([str(ENGINE),'--program',str(path),'--inspect-program'],capture_output=True,text=True)
  self.assertNotEqual(r.returncode,0);self.assertIn('effect or condition',r.stderr)
 def test_evidence_requires_exact_recipe(self):
  p=self.program(DECL+SECOND+THIRD+INITIAL+CONTROL)
  with self.assertRaisesRegex(ValueError,'different typed recipe'):compose(p['recipe'],['Entry'],{'recipe_sha256':'wrong','pairs':[]})
 def test_existing_programs_with_observed_recipe_selection(self):
  active=0
  for name,source in CASES.items():
   with self.subTest(case=name):
    folder=self.root/name;folder.mkdir();path=folder/'source.egg';path.write_text(source)
    core=prepare(path,FRONTEND);base=build(core)
    report=execute(base,folder/'pilot',ENGINE,environment={'CELL_CPU_COMPOSITIONS':'0'})
    plan=compose(base['recipe'],(),{'recipe_sha256':base['recipe']['sha256'],'pairs':report['multi_stage']['executed_rule_pairs']})
    active+=bool(plan['edges']);p=build(core,compositions=plan);path=folder/'program.json';path.write_text(json.dumps(p))
    x=subprocess.run([str(ENGINE),'--program',str(path),'--verify-compositions'],capture_output=True,text=True)
    self.assertEqual(x.returncode,0,x.stderr)
  self.assertGreater(active,5)

if __name__=='__main__':unittest.main()
