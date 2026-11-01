"""Independent native comparison for the standalone Cell CPU executor."""
import argparse,json,subprocess,sys,os
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from eggfuse.engine.cell_runtime import prepare,binary,execute
from eggfuse.ir.cell_runtime import build
from eggfuse.engine.cpu import binary as native_binary,compare_native
from experiments.cell_runtime.audit import check as audit_bindings
from experiments.cell_runtime.congruence_cases import CASES as CONGRUENCE_CASES

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
CASES.update({
 'join-sieve-late':'''(datatype E (N i64) (Seen i64)) (relation P (i64 i64)) (relation Q (i64 i64)) (relation T (i64))
(ruleset a) (ruleset b) (ruleset add)
(rule ((P x y) (Q y z) (T z)) ((Seen x)) :ruleset a)
(rule ((P x y) (Q y z) (T z)) ((N x)) :ruleset b)
(rule ((N 99)) ((T 8)) :ruleset add)
(P 1 11) (P 2 22) (Q 11 7) (Q 22 8) (T 7) (N 99)
(run-schedule (seq (run a) (run b) (run add) (run a) (run b))) (check (Seen 1) (Seen 2) (N 2))''',
 'join-sieve-union':'''(datatype E (N i64) (Seen i64)) (relation P (i64 E)) (relation Q (E E)) (relation T (E))
(ruleset a) (ruleset alias)
(rule ((P n x) (Q x y) (T y)) ((Seen n)) :ruleset a)
(rule ((= a (N 7)) (= b (N 8))) ((union a b)) :ruleset alias)
(P 1 (N 1)) (P 2 (N 2)) (Q (N 1) (N 7)) (Q (N 2) (N 8)) (T (N 7))
(run-schedule (seq (run a) (run alias) (run a))) (check (Seen 1) (Seen 2))''',
 'join-sieve-correlated':'''(datatype E (N i64) (Seen i64)) (relation P (E i64)) (relation Q (E i64 i64)) (relation T (i64))
(ruleset a) (ruleset alias) (ruleset remove)
(rule ((P x n) (Q x n z) (T z)) ((Seen n)) :ruleset a)
(rule ((= a (N 1)) (= b (N 2))) ((union a b)) :ruleset alias)
(rule ((T 99)) ((delete (Q (N 1) 3 99))) :ruleset remove)
(P (N 1) 3) (P (N 1) 7) (Q (N 2) 7 99) (Q (N 1) 3 99) (T 99)
(run-schedule (seq (run remove) (run a) (run alias) (run a))) (check (Seen 7))''',
 'join-sieve-multiple-guards':'''(datatype E (N i64) (Seen i64)) (relation P (i64)) (relation Q (i64 i64 i64)) (relation T (i64)) (relation U (i64))
(ruleset a) (ruleset add)
(rule ((P x) (Q x y z) (T y) (U z)) ((Seen x)) :ruleset a)
(rule ((N 99)) ((U 8)) :ruleset add)
(P 1) (P 2) (Q 1 7 7) (Q 2 7 8) (T 7) (U 7) (N 99)
(run-schedule (seq (run a) (run add) (run a))) (check (Seen 1) (Seen 2))''',
 'join-visible-delete':'''(datatype E (N i64) (Seen i64 i64)) (relation P (i64 i64)) (relation Q (i64 i64)) (relation T (i64))
(ruleset a) (ruleset b) (ruleset remove)
(rule ((P x y) (Q y z) (T z)) ((Seen 1 x)) :ruleset a)
(rule ((P x y) (Q y z) (T z)) ((Seen 2 x)) :ruleset b)
(rule ((N 99)) ((delete (T 7))) :ruleset remove)
(P 1 11) (Q 11 7) (T 7) (T 123) (N 99)
(run-schedule (seq (run a) (run remove) (run b))) (check (Seen 1 1))''',
 'join-refinement-shared':'''(datatype E (N i64) (Seen i64 i64)) (relation P (E i64)) (relation Q (E i64 i64))
(ruleset a) (ruleset b) (ruleset c) (ruleset remove)
(rule ((P x n) (Q x n z)) ((Seen 1 z)) :ruleset a)
(rule ((P x n) (Q x n z)) ((Seen 2 z)) :ruleset b)
(rule ((P x n) (Q x n z)) ((Seen 3 z)) :ruleset c)
(rule ((N 99)) ((delete (Q (N 1) 7 1))) :ruleset remove)
(P (N 1) 7) (Q (N 1) 7 1) (Q (N 1) 8 2) (Q (N 2) 7 4) (N 99)
(run-schedule (seq (run a) (run b) (run remove) (run c))) (check (Seen 1 1) (Seen 2 1))''',
 'join-sieve-cycle':'''(datatype E (N i64) (Seen i64 i64 i64)) (relation P (i64 i64)) (relation Q (i64 i64)) (relation R (i64 i64)) (relation T (i64))
(ruleset a) (ruleset add)
(rule ((P x y) (Q y z) (R z x) (T z)) ((Seen x y z)) :ruleset a)
(rule ((N 99)) ((R 8 2)) :ruleset add)
(P 1 11) (P 2 22) (Q 11 7) (Q 22 8) (R 7 1) (R 8 3) (T 7) (T 8) (N 99)
(run-schedule (seq (run a) (run add) (run a))) (check (Seen 1 11 7) (Seen 2 22 8))''',
 'join-refinement-invalidation':'''(datatype E (N i64) (Seen i64)) (relation P (E i64)) (relation Q (E i64 i64))
(ruleset a) (ruleset add) (ruleset alias)
(rule ((P x n) (Q x n z)) ((Seen z)) :ruleset a)
(rule ((N 99)) ((Q (N 1) 7 3)) :ruleset add)
(rule ((= a (N 1)) (= b (N 2))) ((union a b)) :ruleset alias)
(P (N 1) 7) (Q (N 1) 7 1) (Q (N 1) 8 2) (Q (N 2) 7 4) (N 99)
(run-schedule (seq (run a) (run add) (run a) (run alias) (run a))) (check (Seen 1) (Seen 3) (Seen 4))''',
})
chain='(datatype E (N i64) (P E)) (ruleset build)\n(rule ((= x (N n))) (\n'
for i in range(100):chain+=f'(let p{i} (P '+('x' if i==0 else f'p{i-1}')+'))\n'
CASES['hundred-node-cell']=chain+ ') :ruleset build)\n(N 0) (run build 1)\n'
CASES.update({
 'multistage-local-chain':'''(datatype E (Seed i64) (Leaf i64) (Pair E E) (Mid E i64) (Done E i64))
(ruleset make) (ruleset middle) (ruleset finish)
(rule ((Seed n)) ((Pair (Leaf n) (Leaf (+ n 1)))) :ruleset make)
(rule ((= p (Pair (Leaf a) (Leaf b))) (< a b)) ((Mid p (+ a b))) :ruleset middle)
(rule ((= m (Mid (Pair (Leaf a) (Leaf b)) sum))) ((Done m (+ sum (+ a b)))) :ruleset finish)
(Seed 3) (run-schedule (seq (run make) (run middle) (run finish)))
(check (Done (Mid (Pair (Leaf 3) (Leaf 4)) 7) 14))''',
 'multistage-finite-visibility':'''(datatype E (Seed i64) (Leaf i64) (Pair E E) (Mid E i64) (Done E i64)) (ruleset all)
(rule ((Seed n)) ((Pair (Leaf n) (Leaf (+ n 1)))) :ruleset all)
(rule ((= p (Pair (Leaf a) (Leaf b)))) ((Mid p (+ a b))) :ruleset all)
(rule ((= m (Mid (Pair (Leaf a) (Leaf b)) sum))) ((Done m sum)) :ruleset all)
(Seed 3) (run-schedule (repeat 2 (run all))) (check (Mid (Pair (Leaf 3) (Leaf 4)) 7))''',
 'multistage-alias-alternatives':'''(datatype E (Seed i64) (Leaf i64) (Pair E E) (Seen i64 i64))
(ruleset make) (ruleset alias) (ruleset read)
(rule ((Seed n)) ((Pair (Leaf n) (Leaf (+ n 1)))) :ruleset make)
(rule ((= a (Leaf 0)) (= b (Leaf 1))) ((union a b)) :ruleset alias)
(rule ((Pair (Leaf a) (Leaf b))) ((Seen a b)) :ruleset read)
(Seed 0) (run-schedule (seq (run make) (run alias) (run read)))
(check (Seen 0 0) (Seen 0 1) (Seen 1 0) (Seen 1 1))''',
 'multistage-updated-reply':'''(datatype E (Seed i64) (Leaf i64) (Pair E E) (Seen E i64))
(function Best (E) i64 :merge (min old new)) (ruleset make) (ruleset update) (ruleset read)
(rule ((Seed n)) ((let a (Leaf n)) (Pair a a) (set (Best a) n)) :ruleset make)
(rule ((= a (Leaf n))) ((set (Best a) 1)) :ruleset update)
(rule ((= p (Pair a a)) (= a (Leaf n)) (= cost (Best a))) ((Seen p cost)) :ruleset read)
(Seed 7) (run-schedule (seq (run make) (run update) (run read))) (check (Seen (Pair (Leaf 7) (Leaf 7)) 1))''',
 'multistage-hidden-source':'''(datatype E (Seed i64) (Leaf i64) (Pair E E) (Seen i64)) (ruleset make) (ruleset hide) (ruleset read)
(rule ((Seed n)) ((Pair (Leaf n) (Leaf (+ n 1)))) :ruleset make)
(rule ((= a (Leaf n))) ((subsume (Leaf n))) :ruleset hide)
(rule ((Pair (Leaf a) (Leaf b))) ((Seen (+ a b))) :ruleset read)
(Seed 0) (run-schedule (seq (run make) (run hide) (run read)))''',
 'multistage-late-boundary':'''(datatype E (Seed i64) (Leaf i64) (Pair E E) (Seen i64 i64)) (relation Port (E i64))
(ruleset make) (ruleset late) (ruleset read)
(rule ((Seed n)) ((Pair (Leaf n) (Leaf (+ n 1)))) :ruleset make)
(rule ((= x (Leaf n))) ((Port x n)) :ruleset late)
(rule ((Pair (Leaf a) right) (Port right b)) ((Seen a b)) :ruleset read)
(Seed 2) (run-schedule (seq (run make) (run read) (run late) (run read))) (check (Seen 2 3))''',
 'multistage-function-chain':'''(datatype E (Seed i64) (Leaf i64) (Pair E E) (Done E i64))
(function Selected (E) E :merge old) (function Cost (E) i64 :merge (min old new))
(ruleset make) (ruleset calculate) (ruleset finish)
(rule ((= s (Seed n))) ((set (Selected s) (Pair (Leaf n) (Leaf (+ n 1))))) :ruleset make)
(rule ((= (Pair (Leaf a) (Leaf b)) (Selected s))) ((set (Cost s) (+ a b))) :ruleset calculate)
(rule ((= p (Selected s)) (= c (Cost s))) ((Done p c)) :ruleset finish)
(Seed 4) (run-schedule (seq (run make) (run calculate) (run finish))) (check (Done (Pair (Leaf 4) (Leaf 5)) 9))''',
 'multistage-reentrant-repeat':'''(datatype E (Seed) (Leaf i64) (Step E)) (ruleset init) (ruleset advance)
(rule ((Seed)) ((Step (Leaf 0))) :ruleset init)
(rule ((Step (Leaf n)) (< n 5)) ((Step (Leaf (+ n 1)))) :ruleset advance)
(Seed) (run-schedule (seq (run init) (repeat 2 (run advance)))) (check (Step (Leaf 2)))''',
 'multistage-reentrant-saturate':'''(datatype E (Seed) (Leaf i64) (Step E)) (ruleset init) (ruleset advance)
(rule ((Seed)) ((Step (Leaf 0))) :ruleset init)
(rule ((Step (Leaf n)) (< n 5)) ((Step (Leaf (+ n 1)))) :ruleset advance)
(Seed) (run-schedule (seq (run init) (saturate (run advance)))) (check (Step (Leaf 5)))''',
})
chain28='(datatype E (Seed i64) (Leaf i64) '+ ' '.join(f'(S{i} E E)' for i in range(28))+')\n(ruleset init)\n'
chain28+='(rule ((Seed n)) ((S0 (Leaf n) (Leaf (+ n 1)))) :ruleset init)\n'
for i in range(27):
 chain28+=f'(ruleset stage{i})\n(rule ((S{i} (Leaf a) (Leaf b))) ((S{i+1} (Leaf (+ a 1)) (Leaf (+ b 1)))) :ruleset stage{i})\n'
chain28+='(Seed 0) (Seed 2) (Seed 4)\n(run-schedule (seq (run init) '+' '.join(f'(run stage{i})' for i in range(27))+'))\n(check (S27 (Leaf 27) (Leaf 28)))'
CASES['multistage-28-stages']=chain28
CASES['multistage-nonroot']=CASES['multistage-local-chain'].replace('(Seed i64)', '(Wrap E) (Seed i64)').replace('((Pair (Leaf n) (Leaf (+ n 1)))) :ruleset make)', '((Wrap (Pair (Leaf n) (Leaf (+ n 1))))) :ruleset make)')
CASES['multistage-future-error']='''(datatype E (Seed i64) (Leaf i64) (Pair E E) (Bad i64)) (ruleset make) (ruleset bad)
(rule ((Seed n)) ((Pair (Leaf n) (Leaf (+ n 1)))) :ruleset make)
(rule ((Pair (Leaf a) (Leaf b))) ((Bad (/ 1 0))) :ruleset bad)
(Seed 0) (run-schedule (seq (run make) (run bad :until (Pair (Leaf 0) (Leaf 1)))))'''
CASES.update(CONGRUENCE_CASES)
for name,source in list(CASES.items()):
 for stage in ('a','b','c'):
  source=source.replace(f'(ruleset {stage})',f'(ruleset stage-{stage})').replace(f':ruleset {stage})',f':ruleset stage-{stage})').replace(f'(run {stage})',f'(run stage-{stage})').replace(f'(run {stage} ',f'(run stage-{stage} ')
 CASES[name]=source

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--engine',type=Path);p.add_argument('--cases',nargs='+',choices=sorted(CASES));a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
 engine=a.engine or binary();native_engine=native_binary();summary={}
 for name,source in CASES.items():
  if a.cases and name not in a.cases:continue
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
  if name.startswith('congruence-'):
   execute(program,d/'without-congruence',engine,True,environment={'CELL_CPU_CONGRUENCE':'0'})
   for file in ('state.json','stages.json','binding-audit.json'):
    assert json.loads((d/'cell/runtime'/file).read_text())==json.loads((d/'without-congruence/runtime'/file).read_text()),(name,file)
   c=report['local_congruence']
   if name in ('congruence-unready-stage','congruence-unseen-interior-alias'):assert c['stats']['proven_unions']==0,c
   if name=='congruence-custom-merge':assert not c['enabled'] and not c['reorder_safe'],c
   if os.environ.get('CELL_CPU_CONGRUENCE')=='1' and name in ('congruence-chain','congruence-shared-contexts','congruence-min-merge'):
    assert c['enabled'] and c['stats']['proven_unions']>0,c
   if name=='congruence-chain':
    stopped=execute(program,d/'paused',engine,True,checkpoint_after=1)
    resumed=execute(program,d/'resumed',engine,True,resume=d/'paused/runtime/checkpoint.json')
    assert json.loads((d/'resumed/runtime/state.json').read_text())==json.loads((d/'cell/runtime/state.json').read_text())
    assert resumed['local_congruence']['stats']==c['stats']
    for kind in ('guard','requires','output'):
     bad=json.loads(json.dumps(program));g=bad['congruence']['templates'][0]['groups'][0]
     if kind=='guard':g['guards']=[]
     elif kind=='requires':g['merges'][0]['requires']=[]
     else:g['merges'][0]['right']=999999
     path=d/(kind+'-forged.json');path.write_text(json.dumps(bad))
     rejected=subprocess.run([str(engine),'--program',str(path),'--output',str(d/(kind+'-forged-output'))],capture_output=True,text=True)
     assert rejected.returncode!=0,(kind,rejected.stdout)
  if name=='join-sieve-late' and report['sieve']['enabled'] and not os.environ.get('CELL_CPU_QUERY_INDEX_SCOPE'):
   # Identical fragments in two rulesets share four summaries, not eight.
   assert report['sieve']['stats']['builds']==4, report['sieve']
  if name=='join-refinement-shared' and os.environ.get('CELL_CPU_JOIN_INTERSECTION')=='1' and os.environ.get('CELL_CPU_HASH_BITS')!='0' and not os.environ.get('CELL_CPU_QUERY_INDEX_SCOPE'):
   assert report['join_refinements']['stats']['hits']>0
   assert report['join_refinements']['live_entries_and_references']==[0,0] # deletion invalidated the shared refinement
  if name.startswith('join-'):
   execute(program,d/'without-join',engine,True,environment={'CELL_CPU_JOIN_INTERSECTION':'0','CELL_CPU_SIEVE':'0'})
   for file in ('state.json','stages.json','binding-audit.json'):
    assert json.loads((d/'cell/runtime'/file).read_text())==json.loads((d/'without-join/runtime'/file).read_text()),(name,file)
  if name.startswith('multistage-'):
   execute(program,d/'without-multistage',engine,True,environment={'CELL_CPU_MULTISTAGE':'0'})
   for file in ('state.json','stages.json','binding-audit.json'):
    assert json.loads((d/'cell/runtime'/file).read_text())==json.loads((d/'without-multistage/runtime'/file).read_text()),(name,file)
   if name in ('multistage-local-chain','multistage-function-chain'):
    assert report['multi_stage']['stats']['projected_atoms']>0 and report['multi_stage']['stats']['max_depth']>=2,report['multi_stage']
   if name=='multistage-finite-visibility':assert all(row['table']!='Done' for row in actual['rows'])
   if name=='multistage-28-stages':assert report['multi_stage']['stats']['max_depth']>=27,report['multi_stage']
   if name=='multistage-local-chain':
    # Bypass Python validation to exercise the independent Rust verifier.
    for kind in ('control','projection','point'):
     bad=json.loads(json.dumps(program))
     if kind=='control':bad['multi_stage']['control']=[]
     elif kind=='projection':next(r for r in bad['multi_stage']['routes'] if r['steps'])['steps'][0]['source_index']=999999
     else:bad['multi_stage']['entries'][0]['atoms']=[999999]
     path=d/(kind+'-forged.json');path.write_text(json.dumps(bad))
     rejected=subprocess.run([str(engine),'--program',str(path),'--output',str(d/(kind+'-forged-output'))],capture_output=True,text=True)
     assert rejected.returncode!=0,(kind,rejected.stdout)
  if name in ('math','ordered-merge','non-idempotent','finite','multistage-function-chain','multistage-local-chain','multistage-reentrant-repeat'):
   stopped=execute(program,d/'paused',engine,True,checkpoint_after=2 if name=='multistage-function-chain' else 1)
   assert stopped['status']=='checkpointed'
   resumed=execute(program,d/'resumed',engine,True,resume=d/'paused/runtime/checkpoint.json',extract='Add' if name=='math' else None)
   assert resumed['status']=='complete' and resumed['stats']['bindings']==report['stats']['bindings']
   assert json.loads((d/'resumed/runtime/state.json').read_text())==json.loads((d/'cell/runtime/state.json').read_text())
   if name.startswith('multistage-'):assert resumed['multi_stage']['stats']==report['multi_stage']['stats']
   if name=='math':
    extracted=json.loads((d/'resumed/runtime/extraction.json').read_text())
    assert extracted['roots'] and all(r['node'] is not None for r in extracted['roots'])
    damaged=json.loads((d/'paused/runtime/checkpoint.json').read_text());damaged['watermarks'][0]+=1
    (d/'damaged-checkpoint.json').write_text(json.dumps(damaged))
    attempt=subprocess.run([str(engine),'--program',str(d/'cell/cell-runtime-program.json'),'--resume',str(d/'damaged-checkpoint.json'),'--output',str(d/'damaged-resume')],capture_output=True,text=True)
    assert attempt.returncode!=0 and 'checksum' in attempt.stderr
  summary[name]={'verification':proof,'binding_audit':audited,'replay_exact':True,'checkpoint_checked':name in ('math','ordered-merge','non-idempotent','finite','multistage-function-chain','multistage-local-chain','multistage-reentrant-repeat','congruence-chain'),'stats':report['stats'],'local_congruence':report.get('local_congruence'),'multi_stage':report.get('multi_stage'),'sieve':report.get('sieve'),'join_refinements':report.get('join_refinements'),'live_cells':report['live_cells'],'stored_binding_values':report['stored_binding_values'],'virtual_constructor_facts':report['virtual_constructor_facts']}
  print(name,proof,flush=True)
 (a.output/'results.json').write_text(json.dumps(summary,indent=2)+'\n')
if __name__=='__main__':main()
