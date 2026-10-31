"""Independent native comparison for the standalone Cell CPU executor."""
import argparse,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from eggfuse.engine.cell_runtime import prepare,binary,execute
from eggfuse.ir.cell_runtime import build
from eggfuse.engine.cpu import binary as native_binary,compare_native
from experiments.cell_runtime.audit import check as audit_bindings

CASES={
 'math':(ROOT/'tools/egglog_debugger/fixtures/stage-math.egg').read_text(),
 'finite':'''(datatype E (N i64) (P E) (Q E) (R E)) (ruleset s)
(rule ((= x (N n))) ((P x)) :ruleset s)
(rule ((= x (P n))) ((Q x)) :ruleset s)
(rule ((= x (Q n))) ((R x)) :ruleset s)
(N 0) (run-schedule (repeat 2 (run s))) (check (Q (P (N 0))))''',
 'late-right-delta':'''(datatype E (N i64) (Seen E i64)) (relation Left (E)) (relation Right (E i64))
(ruleset query) (ruleset publish)
(rule ((Left x) (Right x n)) ((Seen x n)) :ruleset query)
(rule ((= x (N n))) ((Right x n)) :ruleset publish)
(Left (N 7)) (run-schedule (seq (run query) (run publish) (run query))) (check (Seen (N 7) 7))''',
 'overlapping-deltas':'''(datatype E (N i64) (Seen E E)) (relation Edge (E E)) (ruleset publish) (ruleset query)
(rule ((= x (N 0)) (= y (N 1)) (= z (N 2))) ((Edge x y) (Edge y z)) :ruleset publish)
(rule ((Edge x y) (Edge y z)) ((Seen x z)) :ruleset query)
(N 0) (N 1) (N 2) (run-schedule (seq (run query) (run publish) (run query))) (check (Seen (N 0) (N 2)))''',
 'union-enables-join':'''(datatype E (N i64) (Seen E)) (relation Left (E)) (relation Right (E)) (ruleset join) (ruleset query)
(rule ((Left x) (Right x)) ((Seen x)) :ruleset query)
(rule ((= x (N 0)) (= y (N 1))) ((union x y)) :ruleset join)
(Left (N 0)) (Right (N 1)) (run-schedule (seq (run query) (run join) (run query))) (check (Seen (N 0)))''',
 'late-union':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset join) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= a (N 0)) (= b (N 1))) ((union a b)) :ruleset join)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (N 1) (run-schedule (repeat 1 (run a) (run join) (run b))) (check (= (Q (P (N 0))) (Q (P (N 1)))))''',
 'function-ports':'''(datatype E (N i64) (P E i64) (Q E i64))
(function Best (E) i64 :merge (min old new)) (ruleset offer) (ruleset a) (ruleset improve) (ruleset b)
(rule ((= x (N n))) ((set (Best x) n)) :ruleset offer)
(rule ((= v (Best x))) ((P x v)) :ruleset a)
(rule ((= x (N n))) ((set (Best x) 1)) :ruleset improve)
(rule ((= v (Best x))) ((Q x v)) :ruleset b)
(N 7) (run-schedule (repeat 1 (run offer) (run a) (run improve) (run b))) (check (P (N 7) 7) (Q (N 7) 1))''',
 'ordered-merge':'''(datatype E (Root)) (datatype C (Pair i64) (Smaller C C) (Seen C))
(function Best (E) C :merge (Smaller old new)) (ruleset a) (ruleset b) (ruleset observe) (ruleset choose)
(rule ((= x (Root))) ((set (Best x) (Pair 7))) :ruleset a)
(rule ((= x (Root))) ((set (Best x) (Pair 5))) :ruleset b)
(rule ((= c (Best x))) ((Seen c)) :ruleset observe)
(rule ((= s (Smaller (Pair a) (Pair b))) (<= a b)) ((union s (Pair a))) :ruleset choose)
(rule ((= s (Smaller (Pair a) (Pair b))) (> a b)) ((union s (Pair b))) :ruleset choose)
(set (Best (Root)) (Pair 10)) (run-schedule (repeat 1 (run a) (run b) (run observe) (saturate (run choose))))
(check (= (Best (Root)) (Pair 5)))''',
 'key-union':'''(datatype E (N i64)) (function Best (E) i64 :merge (min old new)) (ruleset join)
(rule ((= a (N 0)) (= b (N 1))) ((union a b)) :ruleset join)
(set (Best (N 0)) 9) (set (Best (N 1)) 3) (run join 1) (check (= (Best (N 0)) 3))''',
 'set-alias':'''(sort E) (constructor N (i64) E) (sort ES (Set E)) (constructor Box (ES) E)
(ruleset s) (rule ((= a (N 1)) (= b (N 2))) ((union a b)) :ruleset s)
(Box (set-of (N 1) (N 2))) (run s 1)''',
 'container-guard-after-union':'''(sort E) (constructor N (i64) E) (sort ES (Set E)) (constructor Box (ES) E) (constructor Len (i64) E)
(ruleset join) (ruleset inspect)
(rule ((= left (N 1)) (= right (N 2))) ((union left right)) :ruleset join)
(rule ((Box xs)) ((Len (set-length xs))) :ruleset inspect)
(Box (set-of (N 1) (N 2))) (run-schedule (repeat 1 (run inspect) (run join) (run inspect))) (check (Len 1) (Len 2))''',
 'subsume':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset hide) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((subsume (P x))) :ruleset hide)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (run-schedule (repeat 1 (run a) (run hide) (run b)))''',
 'delete':'''(datatype E (N i64) (P E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((delete (P x))) :ruleset b)
(N 0) (run-schedule (repeat 1 (run a) (run b)))''',
 'deleted-parent-binding':'''(datatype E (N i64) (P E i64) (Q i64)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x n)) :ruleset a)
(rule ((P x n)) ((delete (P x n)) (Q n)) :ruleset b)
(N 7) (run-schedule (repeat 1 (run a) (run b))) (check (Q 7))''',
 'non-idempotent':'''(datatype E (N i64)) (relation A (E i64)) (relation B (E i64))
(function Sum (E) i64 :merge (+ old new)) (ruleset a) (ruleset b)
(rule ((A x n)) ((set (Sum x) n)) :ruleset a)
(rule ((B x n)) ((set (Sum x) n)) :ruleset b)
(set (Sum (N 0)) 0) (A (N 0) 1) (B (N 0) 1)
(run-schedule (repeat 2 (run a) (run b))) (check (= (Sum (N 0)) 2))''',
 'same-value-constructor-merge':'''(datatype E (N i64)) (datatype C (Pair i64) (Smaller C C))
(function Best (E) C :merge (Smaller old new)) (ruleset offer)
(rule ((= x (N n))) ((set (Best x) (Pair 1))) :ruleset offer)
(set (Best (N 0)) (Pair 1)) (run offer 1)''',
 'repeat-quiescent-stop':'''(datatype E (N)) (ruleset noop)
(rule ((N)) ((N)) :ruleset noop) (N) (run-schedule (repeat 5 (run noop)))''',
 'until-no-future-error':'''(datatype E (N i64) (P E) (Out i64)) (ruleset init) (ruleset bad)
(rule ((= x (N n))) ((P x)) :ruleset init)
(rule ((P x)) ((Out (/ 1 0))) :ruleset bad)
(N 0) (run init 1) (run-schedule (run bad :until (P (N 0)))) (check (P (N 0)))''',
}
chain='(datatype E (N i64) (P E)) (ruleset build)\n(rule ((= x (N n))) (\n'
for i in range(100):chain+=f'(let p{i} (P '+('x' if i==0 else f'p{i-1}')+'))\n'
CASES['hundred-node-cell']=chain+ ') :ruleset build)\n(N 0) (run build 1)\n'
for name,source in list(CASES.items()):
 for stage in ('a','b','c'):
  source=source.replace(f'(ruleset {stage})',f'(ruleset stage-{stage})').replace(f':ruleset {stage})',f':ruleset stage-{stage})').replace(f'(run {stage})',f'(run stage-{stage})').replace(f'(run {stage} ',f'(run stage-{stage} ')
 CASES[name]=source

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--engine',type=Path);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
 engine=a.engine or binary();native_engine=native_binary();summary={}
 for name,source in CASES.items():
  d=a.output/name;d.mkdir();src=d/'source.egg';src.write_text(source);core=prepare(src);program=build(core)
  report=execute(program,d/'cell',engine,True)
  audited=audit_bindings(program,d/'cell/runtime')
  with (d/'native.log').open('w') as f:subprocess.run([str(native_engine),'--source',str(src.resolve()),'--mode','native','--output',str((d/'native').resolve())],stdout=f,stderr=subprocess.STDOUT,check=True)
  actual=json.loads((d/'cell/runtime/state.json').read_text());reference=json.loads((d/'native/state.json').read_text())
  actual['rows']=[r for r in actual['rows'] if r['table'] not in core.get('assertion_tables',[])]
  proof=compare_native(core,actual,reference)
  native_report=json.loads((d/'native/report.json').read_text())
  assert len(report['steps'])==len(native_report['native_report']['iterations']),(name,len(report['steps']),len(native_report['native_report']['iterations']))
  execute(program,d/'replay',engine,True)
  assert json.loads((d/'cell/runtime/state.json').read_text())==json.loads((d/'replay/runtime/state.json').read_text())
  assert json.loads((d/'cell/runtime/stages.json').read_text())==json.loads((d/'replay/runtime/stages.json').read_text())
  if name in ('math','ordered-merge','non-idempotent','finite'):
   stopped=execute(program,d/'paused',engine,True,checkpoint_after=1)
   assert stopped['status']=='checkpointed'
   resumed=execute(program,d/'resumed',engine,True,resume=d/'paused/runtime/checkpoint.json',extract='Add' if name=='math' else None)
   assert resumed['status']=='complete' and resumed['stats']['bindings']==report['stats']['bindings']
   assert json.loads((d/'resumed/runtime/state.json').read_text())==json.loads((d/'cell/runtime/state.json').read_text())
   if name=='math':
    extracted=json.loads((d/'resumed/runtime/extraction.json').read_text())
    assert extracted['roots'] and all(r['node'] is not None for r in extracted['roots'])
    damaged=json.loads((d/'paused/runtime/checkpoint.json').read_text());damaged['watermarks'][0]+=1
    (d/'damaged-checkpoint.json').write_text(json.dumps(damaged))
    attempt=subprocess.run([str(engine),'--program',str(d/'cell/cell-runtime-program.json'),'--resume',str(d/'damaged-checkpoint.json'),'--output',str(d/'damaged-resume')],capture_output=True,text=True)
    assert attempt.returncode!=0 and 'checksum' in attempt.stderr
  summary[name]={'verification':proof,'binding_audit':audited,'replay_exact':True,'checkpoint_checked':name in ('math','ordered-merge','non-idempotent','finite'),'stats':report['stats'],'live_cells':report['live_cells'],'stored_binding_values':report['stored_binding_values'],'virtual_constructor_facts':report['virtual_constructor_facts']}
  print(name,proof,flush=True)
 (a.output/'results.json').write_text(json.dumps(summary,indent=2)+'\n')
if __name__=='__main__':main()
