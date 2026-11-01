"""Reference marker/storage takeover regressions. No native runtime substitute."""
import json,os,subprocess,tempfile,unittest
from pathlib import Path
from eggfuse.engine.cell_runtime import prepare,execute
from eggfuse.ir.cell_runtime import build,validate
from eggfuse.ir.recipe import digest
from experiments.cell_runtime.check import CASES
from experiments.cell_runtime.audit import check as audit
ROOT=Path(__file__).resolve().parents[2]
FRONTEND=Path(os.environ.get('EGGFUSE_TEST_FRONTEND',str(ROOT/'target/debug/examples/eggfuse_frontend')))
ENGINE=Path(os.environ.get('EGGFUSE_TEST_ENGINE',str(ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime')))
MARKER='''(datatype E (N i64) (F E) (G E) (H E E) (Seen E))
(function View (E E) Unit :merge old)
(ruleset complete) (ruleset consume) (ruleset alias) (ruleset late)
(rule ((= f (F x)) (= g (G x))) ((set (View f g) ())) :ruleset complete)
(rule ((View f g)) ((H f g)) :ruleset consume)
(rule ((= a (N 1)) (= b (N 2))) ((union a b)) :ruleset alias)
(rule ((= h (H f g))) ((Seen h)) :ruleset late)
(F (N 1)) (G (N 1)) (F (N 2)) (G (N 2))
(run-schedule (seq (run complete) (run consume) (run alias) (run complete) (run consume) (run late)))
(check (Seen (H (F (N 1)) (G (N 2)))))'''

def norm(s):return {**s,'rows':sorted(s['rows'],key=lambda r:json.dumps(r,sort_keys=True))}
class ReferenceTests(unittest.TestCase):
 def setUp(self):self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
 def tearDown(self):self.temp.cleanup()
 def program(self,source,markers=()):
  src=self.root/'source.egg';src.write_text(source);p=build(prepare(src,FRONTEND),reference_tables=markers);validate(p);return p
 def pair(self,p,checkpoint=True):
  reports=[]
  for mode in ('0','1'):
   reports.append(execute(p,self.root/mode,ENGINE,audit=True,save_checkpoint=True,environment={'CELL_CPU_REFERENCE_VIEWS':mode}))
  states=[json.loads((self.root/m/'runtime/stages.json').read_text()) for m in ('0','1')]
  self.assertEqual(list(map(norm,states[0])),list(map(norm,states[1])))
  keys=('ordinal','ruleset','path','iterations','epoch','logical_matches','changed')
  self.assertEqual([[s[k] for k in keys] for s in reports[0]['steps']],[[s[k] for k in keys] for s in reports[1]['steps']])
  self.assertEqual(audit(p,self.root/'0/runtime'),audit(p,self.root/'1/runtime'))
  if checkpoint:
   execute(p,self.root/'partial',ENGINE,checkpoint_after=1,environment={'CELL_CPU_REFERENCE_VIEWS':'1'})
   execute(p,self.root/'resume',ENGINE,resume=self.root/'partial/runtime/checkpoint.json',environment={'CELL_CPU_REFERENCE_VIEWS':'1'})
   self.assertEqual(norm(json.loads((self.root/'resume/runtime/state.json').read_text())),norm(states[1][-1]))
  return reports
 def test_marker_multiple_owners_union_late_consumer(self):
  reports=self.pair(self.program(MARKER,['View']))
  self.assertGreater(reports[1]['reference_views']['stats']['absorbed_cells'],0)
  self.assertLess(reports[1]['function_ports'],reports[0]['function_ports'])
  self.assertLess(reports[1]['live_cells'],reports[0]['live_cells'])
 def test_finite_run_and_repeat(self):self.pair(self.program(CASES['multistage-finite-visibility']))
 def test_hidden_and_deleted_inputs(self):
  self.pair(self.program(CASES['multistage-hidden-source']))
 def test_mutable_function_snapshot(self):self.pair(self.program(CASES['multistage-updated-reply']))
 def test_live_function_owner_is_retained(self):
  source='''(datatype E (N i64) (F i64) (G i64)) (function Cost (E) i64 :merge (min old new))
(ruleset make) (ruleset consume)
(rule ((N n)) ((let f (F n)) (set (Cost f) n)) :ruleset make)
(rule ((F n)) ((G n)) :ruleset consume)
(N 7) (run-schedule (seq (run make) (run consume))) (check (= (Cost (F 7)) 7) (G 7))'''
  reports=self.pair(self.program(source));self.assertGreater(reports[1]['reference_views']['stats']['retained_function_owners'],0)
 def test_nonroot_and_partial_cover(self):
  reports=self.pair(self.program(CASES['multistage-nonroot']));self.assertGreater(reports[1]['reference_views']['stats']['uncovered_sources'],0)
 def test_late_boundary(self):self.pair(self.program(CASES['multistage-late-boundary']))
 def test_repeated_absorption(self):self.pair(self.program(CASES['multistage-reentrant-saturate']))
 def test_hide_and_recreate_after_takeover(self):
  source="""(datatype E (N i64) (F E) (H E) (Out i64))
(ruleset grow) (ruleset hide) (ruleset read) (ruleset recreate)
(rule ((= f (F x)) (= x (N n))) ((H f)) :ruleset grow)
(rule ((F x)) ((delete (F x))) :ruleset hide)
(rule ((H (F (N n)))) ((Out n)) :ruleset read)
(rule ((= n (N i))) ((F n)) :ruleset recreate)
(F (N 7)) (run-schedule (seq (run grow) (run hide) (run read) (run recreate) (run grow) (run read)))"""
  reports=self.pair(self.program(source));self.assertGreater(reports[1]['reference_views']['stats']['absorbed_cells'],0)
 def test_compiled_entry_survives_takeover(self):
  source="""(datatype E (Seed i64) (Leaf i64) (Pair E E) (Wrap E) (Out i64))
(ruleset make) (ruleset pack) (ruleset late)
(rule ((= seed (Seed n))) ((Pair seed (Leaf n))) :ruleset make)
(rule ((= seed (Seed n)) (= p (Pair seed (Leaf n)))) ((Wrap p)) :ruleset pack)
(rule ((Pair seed leaf) (= seed (Seed n))) ((Out n)) :ruleset late)
(Seed 3) (run-schedule (seq (run make) (run pack) (run late))) (check (Out 3))"""
  p=self.program(source);reports=self.pair(p,checkpoint=False);self.assertGreater(reports[1]['reference_views']['stats']['recovered_continuations'],0)
  execute(p,self.root/'partial',ENGINE,checkpoint_after=2,environment={'CELL_CPU_REFERENCE_VIEWS':'1'})
  before=json.loads((self.root/'partial/runtime/checkpoint.json').read_text())
  self.assertGreater(reports[1]['multi_stage']['stats']['keyed_reads'],before['continuation_stats']['keyed_reads'])
  self.assertEqual(reports[1]['multi_stage']['stats']['unavailable_sources'],before['continuation_stats']['unavailable_sources'])
  execute(p,self.root/'resume',ENGINE,resume=self.root/'partial/runtime/checkpoint.json',environment={'CELL_CPU_REFERENCE_VIEWS':'1'})
  self.assertEqual(norm(json.loads((self.root/'resume/runtime/state.json').read_text())),norm(json.loads((self.root/'1/runtime/state.json').read_text())))
 def test_original_regression_suite(self):
  for name,source in CASES.items():
   with self.subTest(case=name):
    p=self.program(source);path=self.root/'case.json';path.write_text(json.dumps(p))
    x=subprocess.run([str(ENGINE),'--program',str(path),'--verify-reference-views'],capture_output=True,text=True)
    self.assertEqual(x.returncode,0,x.stderr)
 def test_marker_view_does_not_store_projected_tuple_fields(self):
  p=self.program(MARKER,['View']);execute(p,self.root/'views',ENGINE,save_checkpoint=True,environment={'CELL_CPU_REFERENCE_VIEWS':'1','CELL_CPU_ABSORB':'0'})
  checkpoint=json.loads((self.root/'views/runtime/checkpoint.json').read_text());graph=checkpoint['graph']
  count=0
  for cell in graph['cells'].values():
   if not cell['providers']:continue
   role=p['reference_views']['rules'][cell['template']]
   if role['storage']!='reference-view':continue
   count+=1
   for slot in role['slots']:
    if str(slot['atom']) in cell['providers']:self.assertNotIn(str(slot['slot']),cell['values'])
  self.assertGreater(count,0)
 def test_wrong_slot_certificate_rejected_in_rust(self):
  p=self.program(MARKER,['View']);rule=next(r for r in p['reference_views']['rules'] if r['slots']);rule['slots'][0]['column']+=1
  p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'});path=self.root/'bad.json';path.write_text(json.dumps(p))
  x=subprocess.run([str(ENGINE),'--program',str(path),'--inspect-program'],capture_output=True,text=True);self.assertNotEqual(x.returncode,0);self.assertIn('projection',x.stderr)
 def test_mutable_port_cannot_be_marker(self):
  p=self.program(CASES['multistage-updated-reply'])
  with self.assertRaisesRegex(ValueError,'Unit'):build(p['source_core'],reference_tables=['Best'])
if __name__=='__main__':unittest.main()
