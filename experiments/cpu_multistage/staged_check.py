"""Actual native stage visibility and ordered-merge tests for CPU local patches."""
import argparse,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from eggfuse.engine.cpu import execute,binary,compare_native
from experiments.cpu_multistage.check import CASES as ORIGINAL
CASES={
'finite-stage-visibility':'''(datatype E (N i64) (P E) (Q E) (R E) (Leak))
(ruleset a) (ruleset b) (ruleset c) (ruleset observe)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(rule ((= q (Q p))) ((R q)) :ruleset c)
(rule ((Q p)) ((Leak)) :ruleset observe)
(N 0) (N 1)
(run-schedule (repeat 1 (run a) (run observe) (run b) (run c)))
(fail (check (Leak))) (check (R (Q (P (N 0)))))''',
'same-ruleset-next-run':'''(datatype E (N i64) (P E) (Q E) (R E)) (ruleset s)
(rule ((= x (N n))) ((P x)) :ruleset s)
(rule ((= p (P x))) ((Q p)) :ruleset s)
(rule ((= q (Q p))) ((R q)) :ruleset s)
(N 0) (run-schedule (repeat 2 (run s)))
(check (Q (P (N 0)))) (fail (check (R (Q (P (N 0))))))''',
'finite-repeat-versions':'''(datatype E (N i64) (P E i64) (Q E))
(ruleset grow) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((N (+ n 1))) :ruleset grow)
(rule ((= x (N n))) ((P x (+ n 3))) :ruleset a)
(rule ((= p (P x n))) ((Q p)) :ruleset b)
(N 0) (run-schedule (repeat 3 (run grow) (run a) (run b)))
(check (N 3) (Q (P (N 3) 6))) (fail (check (N 4)))''',
'min-actual-not-proposal':'''(datatype E (N i64) (Mark E i64) (Out E i64))
(function Best (E) i64 :merge (min old new))
(ruleset a) (ruleset b)
(rule ((= x (N n))) ((Mark x n) (set (Best x) n)) :ruleset a)
(rule ((= m (Mark x n)) (= old (Best x))) ((Out m old)) :ruleset b)
(N 7) (set (Best (N 7)) 3)
(run-schedule (repeat 1 (run a) (run b)))
(check (Out (Mark (N 7) 7) 3)) (fail (check (Out (Mark (N 7) 7) 7)))''',
'custom-merge-chain':'''(datatype E (Root))
(datatype T (Term i64))
(datatype C (Pair T i64) (Smaller C C) (Seen C))
(relation Candidate (E T i64))
(function Best (E) C :merge (Smaller old new))
(ruleset offer) (ruleset consume) (ruleset choose)
(rule ((Candidate x t c)) ((set (Best x) (Pair t c))) :ruleset offer)
(rule ((= c (Best x))) ((Seen c)) :ruleset consume)
(rule ((= s (Smaller (Pair a ca) (Pair b cb))) (<= ca cb)) ((union s (Pair a ca))) :ruleset choose)
(rule ((= s (Smaller (Pair a ca) (Pair b cb))) (> ca cb)) ((union s (Pair b cb))) :ruleset choose)
(set (Best (Root)) (Pair (Term 0) 10))
(Candidate (Root) (Term 1) 7) (Candidate (Root) (Term 2) 5)
(run-schedule (repeat 1 (run offer) (run consume) (saturate (run choose))))
(check (= (Best (Root)) (Pair (Term 2) 5))) (check (Seen (Best (Root))))''',
'late-key-union':'''(datatype E (N i64)) (datatype T (Term i64))
(datatype C (Pair T i64) (Smaller C C) (Seen C))
(function Best (E) C :merge (Smaller old new))
(ruleset join) (ruleset choose) (ruleset consume)
(rule ((= left (N 0)) (= right (N 1))) ((union left right)) :ruleset join)
(rule ((= s (Smaller (Pair a ca) (Pair b cb))) (<= ca cb)) ((union s (Pair a ca))) :ruleset choose)
(rule ((= s (Smaller (Pair a ca) (Pair b cb))) (> ca cb)) ((union s (Pair b cb))) :ruleset choose)
(rule ((= c (Best x))) ((Seen c)) :ruleset consume)
(set (Best (N 0)) (Pair (Term 0) 8)) (set (Best (N 1)) (Pair (Term 1) 4))
(run-schedule (repeat 1 (run join) (saturate (run choose)) (run consume)))
(check (= (Best (N 0)) (Pair (Term 1) 4)))''',
'alias-before-consumer':'''(datatype E (N i64) (P E) (Q E))
(ruleset a) (ruleset join) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= left (N 0)) (= right (N 1))) ((union left right)) :ruleset join)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (N 1) (run-schedule (repeat 1 (run a) (run join) (run b)))
(check (= (Q (P (N 0))) (Q (P (N 1)))))''',
'duplicate-local-constructor':'''(datatype E (N i64) (P E) (Q E)) (relation Jobs (E i64))
(ruleset a) (ruleset b)
(rule ((Jobs x n)) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(Jobs (N 0) 1) (Jobs (N 0) 2)
(run-schedule (repeat 1 (run a) (run b))) (check (Q (P (N 0))))''',
'late-function-port':'''(datatype E (N i64) (Mark E i64) (Out E i64))
(function Best (E) i64 :merge (min old new))
(ruleset a) (ruleset adjust) (ruleset b)
(rule ((= x (N n))) ((Mark x n) (set (Best x) n)) :ruleset a)
(rule ((= x (N n))) ((set (Best x) 0)) :ruleset adjust)
(rule ((= m (Mark x n)) (= old (Best x))) ((Out m old)) :ruleset b)
(N 7) (run-schedule (repeat 1 (run a) (run adjust) (run b)))
(check (Out (Mark (N 7) 7) 0)) (fail (check (Out (Mark (N 7) 7) 7)))''',
'versioned-function-ports':'''(datatype E (N i64) (Mark E) (Mid E i64) (Out E i64))
(function Best (E) i64 :merge (min old new))
(ruleset a) (ruleset b) (ruleset adjust) (ruleset c)
(rule ((= x (N n))) ((Mark x) (set (Best x) n)) :ruleset a)
(rule ((= m (Mark x)) (= v (Best x))) ((Mid m v)) :ruleset b)
(rule ((= x (N n))) ((set (Best x) 1)) :ruleset adjust)
(rule ((= mid (Mid m old)) (= m (Mark x)) (= v (Best x))) ((Out mid v)) :ruleset c)
(N 7) (run-schedule (repeat 1 (run a) (run b) (run adjust) (run c)))
(check (Out (Mid (Mark (N 7)) 7) 1)) (fail (check (Out (Mid (Mark (N 7)) 7) 7)))''',
'nonidempotent-multiplicity':'''(datatype E (N i64)) (relation R (E i64))
(function Count () i64 :merge (+ old new)) (ruleset a)
(rule ((R x y)) ((set (Count) 1)) :ruleset a)
(set (Count) 0) (R (N 0) 2) (R (N 0) 3) (R (N 0) 4)
(run a 1)''',
'future-failure-not-early':'''(datatype E (N i64) (P E) (Q E i64))
(ruleset a) (ruleset b) (N 0)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p (/ 1 0))) :ruleset b)
(run a 1) (check (P (N 0))) (run-schedule (run b :until (P (N 0))))''',
'until-boundary':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (run a 1) (run-schedule (run b :until (P (N 0))))
(fail (check (Q (P (N 0)))))''',
}
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--cpu-bin',type=Path);a=p.parse_args()
 engine=a.cpu_bin.resolve() if a.cpu_bin else binary();a.output.mkdir(parents=True,exist_ok=False);summary={}
 for name,source in {**ORIGINAL,**CASES}.items():
  src=a.output/(name+'.egg');src.write_text(source)
  r=execute(src,a.output/name,engine,True,mode='staged',stage_audit=True);s=r['candidate']['staged']
  if name in ('chain','finite-stage-visibility'):assert s['reused_steps']>=4,s
  if name in ('alias-before-consumer','duplicate-local-constructor','late-function-port'):assert s['reused_steps']>0,s
  if name=='future-failure-not-early':assert s['prepared_steps']>0 and s['native_matches']==1,s
  if name=='min-actual-not-proposal':assert s['reused_steps']>0 and s['merge_previews_resolved']>0,s
  if name=='custom-merge-chain':
   merge=[n for n in s['stage_events'][0]['merge_nodes'] if n.get('table')=='Smaller']
   assert len(merge)==2 and merge[1]['args'][0]==merge[0]['local']
   assert s['reused_steps']>0 and len(s['stage_events'][0]['writes'])==2 and s['merge_previews_resolved']>0,s
  if name=='versioned-function-ports':assert s['reused_steps']>=2,s
  if name=='finite-stage-visibility':
   events=s['stage_events'];assert events[0]['source_table_rows']['Q']==0 and events[0]['source_table_rows']['R']==0
   assert events[2]['source_table_rows']['Q']==2 and events[2]['source_table_rows']['R']==0
   assert events[3]['source_table_rows']['R']==2
  if name=='same-ruleset-next-run':assert s['reused_steps']==1 and s['stages']==2,s
  if name=='finite-repeat-versions':
   assert {tuple(e['stage']['iterations']) for e in s['stage_events']} >= {(0,),(1,),(2,)}
   assert s['numeric_reuse']>0
  summary[name]={'verification':r['verification'],'staged':s,'native_seconds':r['native']['inference_seconds'],'staged_seconds':r['candidate']['inference_seconds']}
  print(name,'passed','prepared',s['prepared_steps'],'reused',s['reused_steps'],'merge',s['merge_previews_resolved'],flush=True)
 # Independent process, same file-only artifact and full physical replay.
 for name in ('finite-stage-visibility','custom-merge-chain'):
  out=a.output/(name+'-repeat')
  subprocess.run([str(engine),'--source',str(a.output/(name+'.egg')),'--plan',str(a.output/name/'cpu-program.json'),'--mode','staged','--output',str(out),'--stage-audit'],check=True,stdout=subprocess.DEVNULL)
  assert json.loads((out/'state.json').read_text())==json.loads((a.output/name/'staged/state.json').read_text())
  assert json.loads((out/'report.json').read_text())['staged']==json.loads((a.output/name/'staged/report.json').read_text())['staged']
 # Saved programs are parameterized by facts, not the initial input snapshot.
 unseen=a.output/'unseen.egg';unseen.write_text(ORIGINAL['chain'].replace('(N 0) (N 1)','(N 0) (N 1) (N 99)'))
 r=execute(unseen,a.output/'unseen',engine,True,a.output/'chain/cpu-program.json',mode='staged')
 assert r['candidate']['staged']['reused_steps']==6
 # An error is delayed to the actual stage, and both engines reject it.
 from eggfuse.compiler.targets.cpu import lower
 failure=a.output/'failure.egg';failure.write_text(CASES['future-failure-not-early'].replace('(run-schedule (run b :until (P (N 0))))','(run b 1)'))
 core=a.output/'failure-core.json';subprocess.run([str(engine),'--source',str(failure),'--prepare',str(core)],check=True)
 plan=a.output/'failure-plan.json';plan.write_text(json.dumps(lower(json.loads(core.read_text()))))
 for mode in ('staged','native'):
  cmd=[str(engine),'--source',str(failure),'--mode',mode,'--output',str(a.output/('failure-'+mode))]
  if mode=='staged':cmd+=['--plan',str(plan)]
  result=subprocess.run(cmd,capture_output=True);assert result.returncode!=0 and b'Error:' in result.stderr
 (a.output/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
if __name__=='__main__':main()
