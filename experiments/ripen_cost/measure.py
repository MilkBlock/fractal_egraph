"""Profile the actual CS/CCSS ripen + catalog phase; no trace files or reuse probes."""
import argparse,json,os,pathlib,statistics,subprocess
p=argparse.ArgumentParser();p.add_argument('output',type=pathlib.Path);p.add_argument('--repeats',type=int,default=5);a=p.parse_args()
if a.repeats<1:p.error('--repeats must be positive')
root=pathlib.Path(__file__).resolve().parents[2];a.output.mkdir(parents=True,exist_ok=False)
env=dict(os.environ)
for key in ['EGG_LAYOUT_RIPEN_CONVERGENCE','EGG_LAYOUT_RIPEN_OBSERVE_ONLY','EGG_LAYOUT_RIPEN_PACKET_AUDIT','EGG_LAYOUT_RIPEN_SNAPSHOT_PROBE','EGG_LAYOUT_DEPENDENCY_CONES','EGG_LAYOUT_USE_ONLY']:env.pop(key,None)
env.update(EGG_LAYOUT_RIPEN_JOBS='16',EGG_LAYOUT_RIPEN_PER_BOUNDARY='16',EGG_LAYOUT_RIPEN_ROUNDS='8',EGG_LAYOUT_RIPEN_MILLISECONDS='100000')
cases=[('recursive_cs','experiments/closed_storage/recursive-cs.egg',5),('math4','egglog/tests/math-microbenchmark.egg',4)]
result={'scope':'actual online CS/CCSS phase, source cache retained, intermediate convergence disabled','repeats':a.repeats,'cases':{}}
for name,source,rounds in cases:
 samples=[]
 for i in range(a.repeats+1):
  out=a.output/f'{name}-{i}'
  r=subprocess.run([str(root/'target/release/egg_layout'),'analyze','--recapture-tier0','--source',source,'--rounds',str(rounds),'--output',str(out)],cwd=root,env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True)
  if r.returncode:raise RuntimeError(r.stderr[-4000:])
  q=json.loads((out/'saturated-rule-composition/queue.json').read_text())
  profile=q['profile'];profile['counts']=q['counts'];profile['cache_hits']=sum(j.get('cache_hit') is True for j in q['jobs']);profile['uncached_successes']=sum(j.get('cache_hit') is False for j in q['jobs'])
  if i:samples.append(profile)
 keys=[k for k,v in samples[0].items() if isinstance(v,(int,float))]
 med={k:statistics.median(s.get(k,0) for s in samples) for k in keys}
 ratios={}
 for field in ['initial_engine_seconds','all_engine_creation_seconds','parse_normalize_seconds','setup_seconds','native_saturation_seconds','catalog_compare_seconds','catalog_wall_seconds']:
  ratios[field]=statistics.median(100*s.get(field,0)/s['phase_seconds'] for s in samples)
 bootstrap=lambda s:s.get('initial_engine_seconds',0)+s.get('parse_normalize_seconds',0)+s.get('setup_seconds',0)
 ratios['main_bootstrap']=statistics.median(100*bootstrap(s)/s['phase_seconds'] for s in samples)
 result['cases'][name]={'source':source,'tier0_rounds':rounds,'median':med,'percent_of_phase':ratios,'counts':samples[-1]['counts'],'samples':samples}
 print(name,json.dumps({'median':med,'percent':ratios,'counts':samples[-1]['counts']}),flush=True)
 (a.output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
