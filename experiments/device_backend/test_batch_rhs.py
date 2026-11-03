"""Real device publication regressions; native checks at every Run boundary."""
import argparse,json,hashlib
from pathlib import Path
from run import run
from compare_runs import compare

HERE=Path(__file__).resolve().parent

def tests(build,out):
 out.mkdir(parents=True,exist_ok=True);results={}
 def pair(name,source,**kwargs):
  before=out/(name+'-ordered');after=out/(name+'-batch')
  cached=all((d/'report.json').exists() for d in [before,after])
  if cached:
   a,b=[json.loads((d/'report.json').read_text()) for d in [before,after]]
   for r in [a,b]:
    assert r['source_sha256']==hashlib.sha256(source.read_bytes()).hexdigest()
    assert r['build']==json.loads((build/'build.json').read_text())
  else:a=run(source,build,before,**kwargs);b=run(source,build,after,batch_rhs=True,**kwargs)
  assert a['steps']==b['steps'],name
  result=compare(before,after,device_states=True);result['batch_rhs']=b['batch_rhs'];results[name]=result
  print(name,b['batch_rhs'],flush=True);return b
 for name in ['delta-join','combined','naive','update','snapshot-subsume','set-prediction','deferred-union','ordered-effects','repeated-custom-set','query-value-updates','integer','sets','context-joins']:
  pair(name,HERE/'fixtures'/f'{name}.egg')
 source=out/'duplicates.egg';source.write_text('(relation P (i64)) (relation Q (i64)) (relation Out (i64)) (ruleset r) '+
  ' '.join(f'(P {i}) (Q {i})' for i in range(40))+' (rule ((P x) (Q y)) ((Out x)) :ruleset r) (run r 1) (run r 1)')
 b=pair('duplicates',source,noop_preflight=False);assert b['batch_rhs']['new_rows']==40
 # Hidden constructor keys stay hidden; they are not newly allocated/unhidden.
 source=out/'hidden.egg';source.write_text('(datatype T (N i64) (Box T)) (relation P (T)) (ruleset hide) (ruleset make) '+
  '(let n (N 1)) (Box n) (P n) (rule ((P x)) ((subsume (Box x))) :ruleset hide) '+
  '(rule ((P x)) ((Box x)) :ruleset make) (run hide 1) (run make 1)')
 pair('hidden',source,noop_preflight=False)
 # A newly set constructor requires a separate lookup prediction and union.
 source=out/'prediction-fallback.egg';source.write_text('(datatype T (N i64) (Box T)) (relation P (T)) (ruleset r) '+
  '(let b (N 999)) '+ ' '.join(f'(P (N {i}))' for i in range(256))+' '+
  '(rule ((P x)) ((union b (Box x))) :ruleset r) (rule ((P x)) ((Box x)) :ruleset r) (run r 1)')
 b=pair('prediction-fallback',source,noop_preflight=False);assert b['batch_rhs']['fallback_segments']>0
 source=out/'large.egg';source.write_text('(datatype T (N i64)) (relation Out (i64 i64)) (ruleset r) '+
  ' '.join(f'(N {i})' for i in range(8192))+' (rule ((N n)) ((Out n (+ n 1))) :ruleset r) (run r 1)')
 b=pair('large',source,capacity=32768,match_capacity=16384);assert b['batch_rhs']['new_rows']==8192
 source=out/'mixed-large.egg';source.write_text('(datatype T (N i64)) (relation P (T)) (relation Mark (T)) (relation Next (T)) (function Cost (T) i64 :merge (min old new)) (ruleset r) '+
  ' '.join(f'(P (N {i}))' for i in range(512))+
  ' (rule ((P x)) ((Mark x)) :ruleset r)'+
  ' (rule ((P x) (= z (N 0))) ((union x z)) :ruleset r)'+
  ' (rule ((P x)) ((Next x)) :ruleset r)'+
  ' (rule ((P x) (= x (N n))) ((set (Cost x) n)) :ruleset r) (run r 1)')
 b=pair('mixed-large',source,noop_preflight=False);assert b['batch_rhs']['new_rows']==1024
 source=out/'hidden-large.egg';source.write_text('(datatype T (N i64) (Box T)) (relation P (T)) (ruleset hide) (ruleset make) '+
  ' '.join(f'(P (N {i})) (Box (N {i}))' for i in range(512))+
  ' (rule ((P x)) ((subsume (Box x))) :ruleset hide) (rule ((P x)) ((Box x)) :ruleset make) (run hide 1) (run make 1)')
 b=pair('hidden-large',source,noop_preflight=False);assert b['batch_rhs']['proposals']==512 and b['batch_rhs']['new_rows']==0
 source=out/'multiple-independent.egg';source.write_text('(relation P (i64)) (relation A (i64)) (relation B (i64)) (ruleset r) '+
  ' '.join(f'(P {i})' for i in range(512))+' (rule ((P n)) ((A n) (B (+ n 1))) :ruleset r) (run r 1)')
 b=pair('multiple-independent',source);assert b['batch_rhs']['new_rows']==1024 and b['batch_rhs']['proposals']==1024
 source=out/'proposal-capacity.egg';source.write_text('(relation P (i64)) (relation A (i64)) (relation B (i64)) (relation C (i64)) (ruleset r) '+
  ' '.join(f'(P {i})' for i in range(256))+' (rule ((P n)) ((A n) (B n) (C n)) :ruleset r) (run r 1)')
 b=pair('proposal-capacity',source,match_capacity=512);assert b['batch_rhs']['fallback_segments']==1
 source=out/'dependent-constructors.egg';source.write_text('(datatype T (N i64) (W T) (Box T)) (relation P (T)) (relation Seen (T)) (ruleset r) '+
  ' '.join(f'(P (N {i}))' for i in range(512))+' (rule ((P x)) ((W x) (Box (W x))) :ruleset r) (rule ((P x)) ((Seen x)) :ruleset r) (run r 1)')
 b=pair('dependent-constructors',source);assert b['batch_rhs']['new_rows']==512
 source=out/'split-large.egg';source.write_text('(relation P (i64 i64)) (relation Q (i64 i64)) (relation R (i64 i64)) (relation Out (i64 i64)) (ruleset r) '+
  ' '.join(f'(P {i} {i}) (Q {i} {i}) (R {i} {i})' for i in range(512))+' (rule ((P x a) (Q a b) (R b y)) ((Out x y)) :ruleset r) (run r 1)')
 b=pair('split-large',source,split_matches=True);assert b['batch_rhs']['new_rows']==512 and b['contexts']['emitted']>0
 # Intermediate arithmetic failure must use the original ordered executor;
 # speculative preparation never publishes a partial out-of-order prefix.
 source=out/'overflow.egg';source.write_text('(datatype T (N i64)) (relation Before (i64)) (relation Out (i64)) (ruleset r) '+ ' '.join(f'(N {i})' for i in range(255))+' (N 9223372036854775807) '+
  '(rule ((N n)) ((Before n) (Out (+ n 1))) :ruleset r) (run r 1)')
 for mode in [False,True]:
  try:run(source,build,out/f'overflow-{mode}',batch_rhs=mode,noop_preflight=False)
  except RuntimeError as e:assert 'error=5' in str(e)
  else:raise AssertionError('expected RHS overflow')
 results['overflow-preserves-error']=True
 source=out/'capacity.egg';source.write_text('(datatype T (N i64)) (relation Out (i64)) (ruleset r) '+ ' '.join(f'(N {i})' for i in range(256))+' (rule ((N n)) ((Out n)) :ruleset r) (run r 1)')
 for mode in [False,True]:
  try:run(source,build,out/f'capacity-{mode}',batch_rhs=mode,capacity=300,match_capacity=512,noop_preflight=False)
  except RuntimeError as e:assert 'error=1' in str(e)
  else:raise AssertionError('expected capacity error')
 results['capacity-preserves-error']=True
 (out/'results.json').write_text(json.dumps(results,indent=2));return results

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--build',required=True,type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args();r=tests(a.build,a.output);print('passed',len(r),'cases')
