"""Actual resident GPU block-summary rebuild on/off; no inference omissions.

Uses the established worker/native oracle. Timing and exact stage audit are
separate; one run per variant, with the same shader and worker image.
"""
import argparse,copy,hashlib,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for,CASES as OLD
from verify_native_source import verify
from eggfuse.analyzer.regions import from_core

CASES={
 'late-union':OLD['new-union-match'],
 'output-only':'''(datatype E (N i64)) (function Position () i64 :merge (max old new)) (ruleset s)
(rule ((= i (Position)) (< i 4) (= a (N (* 2 i))) (= b (N (+ 1 (* 2 i)))))
      ((union a b) (set (Position) (+ i 1))) :ruleset s)
(set (Position) 0) (N 0) (N 1) (N 2) (N 3) (N 4) (N 5) (N 6) (N 7)
(run-schedule (saturate (run s)))''',
 'recursive':OLD['recursive'],
 'visibility':OLD['visibility'],
 'function-update':'''(datatype E (N i64) (Pair E E))
(function F (E) E :merge new) (relation Link (E E)) (relation Seen (E E))
(ruleset offer) (ruleset alias) (ruleset read)
(rule ((Link x y)) ((set (F x) y)) :ruleset offer)
(rule ((= a (N 3)) (= b (N 4))) ((union a b)) :ruleset alias)
(rule ((= y (F x))) ((Seen x y)) :ruleset read)
(set (F (N 1)) (N 2)) (Link (N 1) (N 4)) (N 3)
(run-schedule (seq (run read) (run alias) (run offer) (run read)))''',
 'merge-cascade':'''(datatype E (N i64) (K E) (Box E) (Choice E E))
(function F (E) E :merge (Choice old new)) (relation Link (E E)) (relation Seen (E))
(ruleset alias) (ruleset read)
(rule ((Link x y)) ((union x y)) :ruleset alias)
(rule ((= c (Choice a b))) ((union c a) (Seen c)) :ruleset read)
(K (N 0)) (K (N 1)) (Box (K (N 0))) (Box (K (N 1)))
(set (F (K (N 0))) (N 2)) (set (F (K (N 1))) (N 3)) (Link (N 0) (N 1))
(run-schedule (saturate (run alias) (run read)))''',
 'hidden-and-compaction':'''(datatype E (N i64) (K E) (Box E))
(relation Link (E E)) (relation Seen (E)) (ruleset alias) (ruleset read)
(rule ((Link x y)) ((union x y) (subsume (K y))) :ruleset alias)
(rule ((= k (K x))) ((Seen k)) :ruleset read)
(K (N 0)) (K (N 1)) (Box (N 9)) (Link (N 0) (N 1))
(run-schedule (seq (run read) (run alias) (run read)))''',
 'container':'''(sort E) (constructor N (i64) E) (sort ES (Set E)) (constructor Box (ES) E)
(relation Link (E E)) (relation Seen (i64)) (ruleset alias) (ruleset read)
(rule ((Link x y)) ((union x y)) :ruleset alias)
(rule ((Box xs)) ((Seen (set-length xs))) :ruleset read)
(Box (set-of (N 0) (N 1))) (Box (set-of (N 0))) (Link (N 0) (N 1))
(run-schedule (seq (run read) (run alias) (run read)))''',
}


# Evacuate whole physical blocks, refill them with Unit-returning rows whose
# equality keys differ, then rename that new key. Scalar/Unit writes must not
# accidentally inherit a full-length summary from the evacuated block.
CASES['compact-regrow'] = '''(datatype E (N i64))
(relation R (E i64)) (relation S (E i64)) (relation Seen (i64))
(ruleset first) (ruleset grow) (ruleset second) (ruleset read)
(rule ((= a (N 0)) (= b (N 1))) ((union a b)) :ruleset first)
(rule ((R (N 0) i)) ((S (N 3) i)) :ruleset grow)
(rule ((= a (N 2)) (= b (N 3))) ((union a b)) :ruleset second)
(rule ((S (N 2) i)) ((Seen i)) :ruleset read)
(N 0) (N 1) (N 2) (N 3)
'''+''.join(f'(R (N 0) {i}) (R (N 1) {i})\n' for i in range(600))+'''
(run-schedule (seq (run first) (run grow) (run second) (run read)))'''

def sparse_source(nodes,steps):
 return '''(datatype E (N i64)) (relation Link (i64 E E)) (relation Tick (i64)) (ruleset s)
(rule ((Tick i) (Link i x y)) ((union x y) (Tick (+ i 1))) :ruleset s)
(Tick 0)
'''+''.join(f'(N {i})\n' for i in range(nodes))+''.join(f'(Link {i} (N {2*i}) (N {2*i+1}))\n' for i in range(steps))+'(run-schedule (saturate (run s)))'

SEMANTIC=('rows','parents','fact_parents','sets','watermarks','meta','history','state','completed')

def check(build,out,only=None,bench=False,nodes=8192,steps=64,pass3=None):
 out.mkdir(parents=True,exist_ok=False);cases={'sparse':sparse_source(nodes,steps)} if bench else {**CASES,'sparse':sparse_source(1024,8)}
 capacities={}
 if pass3:
  if not bench:raise ValueError('Pass 3 uses clean on/off timing; native coverage is tested separately')
  suite=ROOT/'experiments/profile_suite';manifest=json.loads((suite/'suite.json').read_text());cases={}
  for name in pass3:
   spec=next(c for c in manifest['cases'] if c['name']==name)
   data=b''.join((suite/p).read_bytes() for p in spec['parts']);assert hashlib.sha256(data).hexdigest()==spec['sha256']
   cases[name]=data.decode();capacities[name]=(spec['capacity'],spec['match_capacity'])
 if only:cases={k:cases[k] for k in only}
 original=runtime.lower;results={}
 for name,source in cases.items():
  src=out/(name+'.egg');src.write_text(source);core=core_for(src,build);plan=from_core(core);initial=core.pop('initial')
  lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
  modes=['off','on'] if bench else ['off','on','collision']
  runs={}
  for mode in modes:
   def lower(*args,**kw):
    p=original(*args,**kw,incremental_rebuild=(mode!='off'))
    p['incremental_rebuild']['force_collisions']=mode=='collision'
    return p
   runtime.lower=lower
   try:
    if bench and pass3 and mode=='off':
     # First shader compilation/first launch is a preflight, never the timed
     # control. Both timed modes now use the same warmed binary archive.
     warm=runtime.run_library(lib,build,out/(name+'-warmup'),*capacities[name],region_program=plan,audit_numeric=False)
     assert warm['complete']
    runs[mode]=runtime.run_library(lib,build,out/(name+'-'+mode),*capacities.get(name,(max(4096,2*len(initial)),max(4096,2*steps))),region_program=plan,audit_numeric=not bench)
   finally:runtime.lower=original
   assert runs[mode]['complete'],(name,mode,runs[mode].get('rejected'))
  states={mode:r['programs'][0]['instances'][0] for mode,r in runs.items()}
  for mode in modes[1:]:
   for field in SEMANTIC:assert states['off'][field]==states[mode][field],(name,mode,field)
  shaders=[(out/(name+'-'+m)/'program-0/attempt-0/kernel.metal').read_text() for m in modes]
  assert all(s==shaders[0] for s in shaders)
  native=None
  if not bench:native=verify(src,build,out/(name+'-on')/'program-0/cell-0',out/(name+'-native'))
  if bench and pass3:
   for field in SEMANTIC:assert warm['programs'][0]['instances'][0][field]==states['off'][field]
  if name=='output-only':assert states['on']['incremental_rebuild']['duplicate_group_rows_skipped']>0
  if name=='sparse':assert states['on']['incremental_rebuild']['counters']['blocks_skipped']>0
  result=dict(scope='actual Metal resident engine; same shader/worker; original finite and saturation schedule',native=native,
     source_sha256=hashlib.sha256(source.encode()).hexdigest(),shader_sha256=hashlib.sha256(shaders[0].encode()).hexdigest(),pass3=bool(pass3),
     warmup_excluded=bool(bench and pass3),exact_state_and_history_equal=True,initial_rows=len(initial),runs=len(states['on']['history']),
     seconds={m:{'inference':r['programs'][0]['inference_seconds'],'gpu':r['programs'][0]['gpu_seconds']} for m,r in runs.items()},
     statistics={m:s['incremental_rebuild'] for m,s in states.items()},worker_provenance=runs['on']['worker_provenance'],audit=not bench)
  results[name]=result;(out/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(name,json.dumps(result),flush=True)
 return results

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--cases',nargs='+');p.add_argument('--bench',action='store_true');p.add_argument('--nodes',type=int,default=8192);p.add_argument('--steps',type=int,default=64);p.add_argument('--pass3',nargs='+',choices=['fact','loopfact','sum-bits','raytrace'])
 a=p.parse_args();check(a.build.resolve(),a.output.resolve(),a.cases,a.bench,a.nodes,a.steps,a.pass3)
