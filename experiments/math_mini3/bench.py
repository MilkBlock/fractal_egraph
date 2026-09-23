#!/usr/bin/env python3
"""One run per mode. External 10s/4GiB process-group guard, not inference policy."""
import pathlib,subprocess,time,os,signal,json,re,hashlib,argparse
p=argparse.ArgumentParser();p.add_argument('--output',required=True);p.add_argument('--seconds',type=float,default=10);p.add_argument('--bounded-only',action='store_true');p.add_argument('--no-svg',action='store_true');p.add_argument('--default-only',action='store_true');p.add_argument('--no-graphs',action='store_true');p.add_argument('--compare-native',action='store_true');p.add_argument('--sample',action='store_true');p.add_argument('--profile',action='store_true');a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];out=(root/a.output);out.mkdir(parents=True,exist_ok=False)
source=root/'experiments/math_mini3/source.egg';binary=root/'target/release/egg_layout'
report={'commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'environment_overrides':{k:os.environ[k] for k in ['EGG_LAYOUT_ANALYSIS_THREADS','EGG_LAYOUT_QUERY_THREADS','RAYON_NUM_THREADS','EGG_LAYOUT_FULL_TRACE','EGG_LAYOUT_RIPEN_HISTORY'] if k in os.environ},'sampling':a.sample,'phase_profiling':a.profile,'graphs':not a.no_graphs,'eager_svg':not (a.no_svg or a.no_graphs),'external_guard':{'seconds':a.seconds,'group_rss_bytes':4*1024**3},'runs':[]}
for name,cmd in [
 ('native',[str(root/'target/release/examples/native_math_baseline'),str(source)]),
 ('analysis-default',[str(binary),'analyze','--source',str(source),'--output',str(out/'default')]),
 ('analysis-50ms-200k',[str(binary),'analyze','--source',str(source),'--output',str(out/'bounded'),'--ripen-observe-ms','50','--ripen-observe-facts','200000'])]:
 if a.compare_native and name=='analysis-50ms-200k':continue
 if a.bounded_only and name!='analysis-50ms-200k':continue
 if a.default_only and name!='analysis-default':continue
 env=os.environ.copy()
 for key in ['EGG_LAYOUT_RIPEN_OBSERVE_MS','EGG_LAYOUT_RIPEN_OBSERVE_FACTS','EGG_LAYOUT_RIPEN_SWEEP_STEPS']:env.pop(key,None)
 env['EGG_LAYOUT_GRAPHS']='0' if a.no_graphs else '1'
 if a.profile:env['EGG_LAYOUT_PROFILE']='1'
 else:env.pop('EGG_LAYOUT_PROFILE',None)
 env['EGG_LAYOUT_EAGER_SVG']='0' if a.no_svg else '1';samples=[];reason=None;sampler=None;begin=time.monotonic()
 with (out/(name+'.stdout')).open('w') as stdout,(out/(name+'.stderr')).open('w') as stderr:
  child=subprocess.Popen(['/usr/bin/time','-l',*cmd],cwd=root,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
  while child.poll() is None:
   elapsed=time.monotonic()-begin;ps=subprocess.check_output(['ps','-axo','pid=,pgid=,rss=,comm='],text=True)
   group=[x.split(None,3)for x in ps.splitlines() if len(x.split(None,3))==4 and x.split(None,3)[1]==str(child.pid)]
   rss=sum(int(x[2])*1024 for x in group);samples.append({'seconds':elapsed,'rss_bytes':rss,'processes':[{'pid':int(x[0]),'rss_bytes':int(x[2])*1024,'name':pathlib.Path(x[3]).name}for x in group]})
   if a.sample and name!='native' and sampler is None and (rss>=512*1024**2 or elapsed>=5):
    engines=[x for x in group if pathlib.Path(x[3]).name=='egg_layout']
    if engines:sampler=subprocess.Popen(['sample',engines[0][0],'1','-file',str(out/(name+'.sample.txt'))],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
   if rss>=4*1024**3 or elapsed>=a.seconds:
    reason='external_rss_guard' if rss>=4*1024**3 else 'external_time_guard'
    try:os.killpg(child.pid,signal.SIGKILL)
    except ProcessLookupError:pass
    child.wait();break
   try:child.wait(timeout=.1)
   except subprocess.TimeoutExpired:pass
 elapsed_process=time.monotonic()-begin
 if sampler is not None:sampler.wait(timeout=10)
 text=(out/(name+'.stderr')).read_text();peak=re.search(r'(\d+)  maximum resident set size',text);native=re.search(r'baseline_native_seconds=([\d.]+)',text);real=re.search(r'([\d.]+) real',text)
 r={'name':name,'command':cmd,'elapsed_seconds':elapsed_process,'time_real_seconds':float(real[1])if real else None,'exit_code':child.returncode,'stop_reason':reason,'sampled_peak_group_rss_bytes':max((s['rss_bytes']for s in samples),default=0),'time_peak_process_rss_bytes':int(peak[1])if peak else None,'native_seconds':float(native[1])if native else None,'samples':samples}
 report['runs'].append(r);(out/'measurements.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items()if k not in ['samples','command']}),flush=True)
