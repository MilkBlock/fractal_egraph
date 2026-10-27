"""Saturate-level matrix execution versus original GPU and independent native."""
import argparse,itertools,json,math
from pathlib import Path
from run import run
from compare_runs import compare
from matrix_sat import separator


def tests(build,out):
 out.mkdir(parents=True,exist_ok=False);results={}
 def pair(name,source,**kwargs):
  file=out/(name+'.egg');file.write_text(source)
  before=out/(name+'-normal');after=out/(name+'-matrix')
  a=run(file,build,before,**kwargs);b=run(file,build,after,matrix_saturate=True,**kwargs)
  fields=['set','rows','matched','updated','epoch','unions']
  assert [[x[k] for k in fields] for x in a['steps']]==[[x[k] for k in fields] for x in b['steps']],name
  r=compare(before,after);r['device_bijection_check']=compare(before,after,device_states=True,semantic_device=True)['final_check'];r['calls']=b['matrix_saturate']['calls'];results[name]=r
  print(name,[(x['logical_runs'],x['published_rows']) for x in r['calls']],flush=True);return b
 for k in range(9):
  m=separator(k)
  for bits in itertools.product([0,1],repeat=k):assert (m['bias']+m['coefficient']*sum(bits)>m['threshold'])==all(bits)
 results['separator-certificate']=True
 common='''(datatype T (N i64) (Edge T T))
(relation P (T)) (relation Seed (T))
(ruleset reach)
(rule ((Seed x)) ((P x)) :ruleset reach)
(rule ((Edge x y) (P x)) ((P y)) :ruleset reach)
'''
 source=common+'''(Seed (N 0)) (Edge (N 0) (N 1)) (Edge (N 1) (N 2)) (Edge (N 2) (N 3))
(relation Early ()) (ruleset observe)
(rule ((P (N 2))) ((Early)) :ruleset observe)
(run reach 1) (run observe 1)
(run-schedule (saturate (run reach)))
(run observe 1) (check (P (N 3)))
'''
 b=pair('finite-run-boundary',source);assert b['steps'][1]['matched']==0 and b['matrix_saturate']['calls'][0]['logical_runs']==4
 b=pair('unseeded-cycle',common+'(Edge (N 0) (N 1)) (Edge (N 1) (N 0)) (run-schedule (saturate (run reach)))')
 assert b['matrix_saturate']['calls'][0]['published_rows']==0
 source=common+'''(Seed (N 0)) (Edge (N 0) (N 1)) (P (N 0))
(ruleset hide) (rule ((Seed x)) ((subsume (P x))) :ruleset hide)
(run hide 1) (run-schedule (saturate (run reach)))
'''
 b=pair('hidden-output',source);assert b['matrix_saturate']['calls'][0]['published_rows']==0
 source=common+'''(Seed (N 0)) (Edge (N 1) (N 2))
(ruleset merge) (rule ((= x (N 0)) (= y (N 1))) ((union x y)) :ruleset merge)
(run-schedule (saturate (run reach))) (run merge 1)
(run-schedule (saturate (run reach))) (check (P (N 2)))
'''
 b=pair('union-between-cells',source);assert len(b['matrix_saturate']['calls'])==2
 source='''(datatype T (N i64) (Pair T T)) (relation P (T)) (relation Q (T)) (relation Seed (T)) (ruleset r)
(rule ((Seed x)) ((P x)) :ruleset r)
(rule ((= p (Pair x y)) (P x) (P y)) ((P p) (Q y)) :ruleset r)
(Seed (N 0)) (Seed (N 1)) (Pair (N 0) (N 1)) (Pair (Pair (N 0) (N 1)) (N 1))
(run-schedule (saturate (run r))) (check (Q (N 1)))
'''
 b=pair('branching-multiple-outputs',source);assert b['matrix_saturate']['calls'][0]['published_rows']==5
 source=common+'''(Seed (N 0)) (Edge (N 0) (N 1)) (Edge (N 1) (N 2))
(ruleset hide) (rule ((= x (N 0)) (= y (N 1))) ((subsume (Edge x y))) :ruleset hide)
(run hide 1) (run-schedule (saturate (run reach)))
'''
 b=pair('hidden-anchor',source);assert b['matrix_saturate']['calls'][0]['published_rows']==1
 source='''(datatype T (N i64) (Link T T)) (relation P (T)) (ruleset r) (ruleset merge)
(rule ((Link x x)) ((P x)) :ruleset r)
(rule ((= a (N 0)) (= b (N 1))) ((union a b)) :ruleset merge)
(Link (N 0) (N 1)) (run-schedule (saturate (run r))) (run merge 1) (run-schedule (saturate (run r)))
'''
 b=pair('alias-enables-marker',source);assert [x['published_rows'] for x in b['matrix_saturate']['calls']]==[0,1]
 source='''(datatype T (N i64)) (relation P (T)) (ruleset r)
(rule ((= x (N n)) (< n 3)) ((P x)) :ruleset r)
(N 0) (N 3) (run-schedule (saturate (run r)))
'''
 b=pair('guarded-fallback',source);assert not b['matrix_saturate']['calls']
 source='''(datatype T (N i64) (Text String)) (relation P (T)) (ruleset r)
(rule ((= x (N 3))) ((P x)) :ruleset r)
(rule ((= x (Text "yes"))) ((P x)) :ruleset r)
(N 2) (N 3) (Text "yes") (Text "no") (run-schedule (saturate (run r)))
'''
 b=pair('typed-anchor-literals',source);assert b['matrix_saturate']['calls'][0]['published_rows']==2
 source='''(datatype T (N i64) (Edge T T)) (relation P (T)) (relation Q (T)) (relation Seed (T)) (ruleset a) (ruleset b)
(rule ((Seed x)) ((P x)) :ruleset a)
(rule ((Edge x y) (P x)) ((Q y)) :ruleset a)
(rule ((Edge x y) (Q x)) ((P y)) :ruleset b)
(unstable-combined-ruleset both a b)
(Seed (N 0)) (Edge (N 0) (N 1)) (Edge (N 1) (N 2))
(run a 1) (run-schedule (saturate (run both))) (run a 1) (run b 1)
'''
 b=pair('combined-rule-watermarks',source);assert b['matrix_saturate']['calls'][0]['published_rows']==2
 source='''(datatype T (N i64) (Quad T T T T)) (relation P (T)) (relation Q (T)) (relation Seed (T)) (ruleset r)
(rule ((Seed x)) ((P x)) :ruleset r)
(rule ((= t (Quad a b c d)) (P a) (P b) (P c) (P d)) ((Q t)) :ruleset r)
'''
 for i,bits in enumerate(itertools.product([False,True],repeat=4)):
  nodes=[f'(N {i*4+j})' for j in range(4)];source+='(Quad '+' '.join(nodes)+')\n'
  source+=' '.join(f'(P {n})' for n,bit in zip(nodes,bits) if bit)+'\n'
 source+='(run-schedule (saturate (run r)))'
 b=pair('all-four-port-assignments',source);assert b['matrix_saturate']['calls'][0]['published_rows']==1
 source=common+'(Seed (N 0)) '+' '.join(f'(Edge (N {i}) (N {i+1}))' for i in range(96))+' (run-schedule (saturate (run reach))) (check (P (N 96)))'
 b=pair('deep-closure',source);assert b['matrix_saturate']['calls'][0]['logical_runs']==98
 # A macro checkpoint is a normal public boundary. Restoring it must not
 # reinterpret private local iterations as new global work.
 resumed=run(out/'deep-closure.egg',build,out/'deep-closure-resumed',resume=out/'deep-closure-matrix',matrix_saturate=True)
 assert resumed['resumed_prefix_runs']==len(b['steps']) and resumed['complete_schedule']
 results['macro-checkpoint-resume']=True
 b=pair('debug-prefix-keeps-run-semantics',source,max_runs=3);assert not b['matrix_saturate']['calls'] and not b['complete_schedule']
 # Queue/table exhaustion never accepts a truncated private closure.
 file=out/'capacity.egg';file.write_text(common+'(Seed (N 0)) (Edge (N 0) (N 1)) (run-schedule (saturate (run reach)))')
 for mode in [False,True]:
  try:run(file,build,out/f'capacity-{mode}',capacity=5,match_capacity=32,matrix_saturate=mode)
  except RuntimeError as e:assert 'error=1' in str(e)
  else:raise AssertionError('expected storage exhaustion')
 results['capacity-fallback']=True
 (out/'results.json').write_text(json.dumps(results,indent=2));return results

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();r=tests(a.build,a.output);print('passed',len(r),'cases')
