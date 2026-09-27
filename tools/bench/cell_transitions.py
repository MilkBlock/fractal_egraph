#!/usr/bin/env python3
"""Same frozen RipSatCell library: original matching versus compiled residual views."""
import argparse,hashlib,json,os,statistics,subprocess
from pathlib import Path
from math_virtual import ROOT,measure
from math_graph_equal import verify
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--output',required=True);p.add_argument('--runs',type=int,default=3)
p.add_argument('--binary',default='target/release/examples/adaptive_math');p.add_argument('--profile',action='store_true')
p.add_argument('--instances',type=int,default=64);p.add_argument('--fanout',type=int,default=32)
a=p.parse_args()
if min(a.runs,a.instances,a.fanout)<1:p.error('counts must be positive')
out=ROOT/a.output;out.mkdir(parents=True,exist_ok=False);binary=ROOT/a.binary
os.environ.update(EGG_LAYOUT_CELL_EVOLUTION='0',EGG_LAYOUT_SKIP_INTERNAL_RHS='0',EGG_LAYOUT_PRUNE_INTERNAL_JOIN='0',EGG_LAYOUT_AUDIT_ENCLOSURES='0',EGG_LAYOUT_MIN_TEMPLATE_REUSE='2',EGG_LAYOUT_JOIN_READ_PROFILE=str(int(a.profile)))
r={'parent_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'working_tree_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)), 'scope':'Actual native runtime, identical frozen library and source, no logical tracing. Both timers include installation, coverage preparation and packing. Profile counters include helper tables. Math snapshots exclude optimizer-owned functions, which are metadata not constructors.','binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'profile':a.profile,'threads':os.environ.get('RAYON_NUM_THREADS','auto'),'cases':{}}
def invoke(mode,src,lib,dest,spec,verification=False):
 os.environ['EGG_LAYOUT_AUDIT_VERIFY']=str(int(verification))
 os.environ.pop('EGG_LAYOUT_CELL_TRANSITIONS',None)
 if mode=='residual':os.environ['EGG_LAYOUT_CELL_TRANSITIONS']=str(spec)
 return measure([binary,'frozen',src,dest,lib],Path(str(dest)+'-process'))
for case,depth in [('one_atom',1),('nested',4),('math6',0)]:
 work=out/case;work.mkdir();src=work/'source.egg';spec=work/'transitions.json'
 if depth:
  ops=[f'N{i}' for i in range(depth)];term='x'
  for op in reversed(ops):term=f'({op} {term})'
  env='(datatype Math (V i64) (A Math) (Flag Math) (Pair Math Math) (Done Math Math) '+' '.join(f'({op} Math)' for op in ops)+')\n'
  env+=f'(rewrite (A x) {term})\n(rewrite (Pair {term} (Flag y)) (Done x y))\n'
  train=work/'train.egg';train.write_text(env+f'(A (V 99999))\n(Pair {term.replace("x","(V 99999)")} (Flag (V 99998)))\n(run 2)\n')
  os.environ.pop('EGG_LAYOUT_CELL_TRANSITIONS',None);os.environ['EGG_LAYOUT_AUDIT_VERIFY']='0'
  measure([binary,'adaptive',train,work/'bake'],work/'bake-process')
  lib=work/'bake/bake_lib_round1.json'
  seed='\n'.join(f'(let seed{i} (A (V {i})))' for i in range(a.instances))
  graft='\n'.join(f'(Pair seed{i} (Flag (V {100000+j})))' for i in range(a.instances) for j in range(a.fanout))
  src.write_text(env+seed+'\n(run 2)\n'+graft+'\n(run 2)\n')
  plan=[dict(source_rule=0,consumer_rule=1,cut_path=[0],anchor_path=[0]*(depth-1))]
 else:
  src.write_text((ROOT/'experiments/math_mini3/source.egg').read_text().replace('(run 3)','(run 6)'))
  lib=ROOT/'experiments/math_mini3/adaptive_runtime/bake_lib_round2.json'
  plan=[dict(source_rule=0,consumer_rule=2,cut_path=[1],anchor_path=[])]
 spec.write_text(json.dumps(plan,indent=2)+'\n')
 for mode in ['control','residual']:invoke(mode,src,lib,work/('verify-'+mode),spec,True)
 result={'library_bytes':lib.stat().st_size,'compiled_plan_bytes':(work/'verify-residual/cell-transitions.json').stat().st_size,'generated_program_bytes':(work/'verify-residual/cell-transitions.egg').stat().st_size,'source_sha256':hashlib.sha256(src.read_bytes()).hexdigest(),'library_sha256':hashlib.sha256(lib.read_bytes()).hexdigest(),'verification':verify(work/'verify-control/verification-graph.json',work/'verify-residual/verification-graph.json'),'runs':[]}
 for i in range(a.runs):
  for mode in (['control','residual'] if i%2==0 else ['residual','control']):
   x=invoke(mode,src,lib,work/f'{mode}-{i}',spec);full=x['result']
   x['result']={k:full[k] for k in ['seconds','execution_seconds','matches','final','join_reads']}
   x['result']['fallback_matches']=sum(s.get('cell_transition',{}).get('fallback_matches',0) for s in full['samples'] if s.get('cell_transition'))
   x['result']['interface_snapshots']=[s['cell_transition'] for s in full['samples'] if s.get('cell_transition')]
   x['result']['prepare_seconds']=sum(s.get('transition_prepare_seconds',0) for s in full['samples'])
   if a.profile and full['join_reads'] is None:raise RuntimeError('profile build required: --features native-read-profile')
   x.update(mode=mode,repeat=i);result['runs'].append(x)
 result['medians']={m:{k:statistics.median(x['result'][k] for x in result['runs'] if x['mode']==m) for k in ['seconds','execution_seconds','matches','fallback_matches','prepare_seconds']} for m in ['control','residual']}
 for mode in ['control','residual']:
  result['medians'][mode]['peak_rss_bytes']=statistics.median(x['peak_rss_bytes'] for x in result['runs'] if x['mode']==mode)
 result['proof']=json.loads((work/'verify-residual/cell-transitions.json').read_text())
 r['cases'][case]=result;(out/'report.json').write_text(json.dumps(r,indent=2)+'\n')
 print(case,json.dumps({'verification':result['verification'],'medians':result['medians']}),flush=True)
