#!/usr/bin/env python3
"""External measurement only; never changes inference or saturation semantics."""
import argparse, hashlib, json, os, pathlib, signal, subprocess, time
p=argparse.ArgumentParser();p.add_argument('--output',required=True);p.add_argument('--observe-seconds',type=float,default=180);p.add_argument('--rounds',type=int);p.add_argument('--mode',choices=['full','trace'],default='full');a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];out=root/a.output;out.mkdir(parents=True,exist_ok=False)
source=root/'egglog/tests/math-microbenchmark.egg'
meta={'commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'source':str(source.relative_to(root)),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'observation_seconds':a.observe_seconds,'kernel':'local patched egglog, no trace baseline; not pristine upstream','machine':subprocess.check_output(['uname','-a'],text=True).strip(),'source_files':{str(f):hashlib.sha256((root/f).read_bytes()).hexdigest() for f in ['src/native_analyze.rs','src/native_saturated_rule_composition_pipeline.rs']},'runs':[]}
def save(): (out/'measurement.json').write_text(json.dumps(meta,indent=2)+'\n')
def run(name,command,deadline):
    start=time.monotonic();samples=[];reason=None
    with (out/(name+'.stdout')).open('w') as stdout,(out/(name+'.stderr')).open('w') as stderr:
        child=subprocess.Popen(['/usr/bin/time','-l',*command],cwd=root,stdout=stdout,stderr=stderr,start_new_session=True)
        while child.poll() is None:
            elapsed=time.monotonic()-start
            ps=subprocess.check_output(['ps','-axo','pid=,pgid=,rss=,%cpu=,comm='],text=True)
            group=[line.split(None,4) for line in ps.splitlines() if len(line.split(None,4))==5 and line.split(None,4)[1]==str(child.pid)]
            samples.append({'seconds':elapsed,'rss_kib':sum(int(x[2]) for x in group),'cpu_percent':sum(float(x[3]) for x in group),'processes':[x[4] for x in group]})
            if elapsed>=deadline:
                reason='external_observation_window_ended; not a saturation or infinity verdict'
                os.killpg(child.pid,signal.SIGKILL);child.wait();break
            try: child.wait(timeout=1)
            except subprocess.TimeoutExpired: pass
    result={'name':name,'command':command,'wall_seconds':time.monotonic()-start,'exit_code':child.returncode,'stop_reason':reason,'sampled_peak_group_rss_kib':max((x['rss_kib'] for x in samples),default=0),'samples':samples}
    meta['runs'].append(result);save();print(json.dumps({k:v for k,v in result.items() if k!='samples'}),flush=True)
if a.mode=='trace':
    run('trace-11',[str(root/'target/release/examples/native_math_baseline'),str(source),'trace'],a.observe_seconds)
else:
    for i in range(3):run(f'baseline-{i+1}',[str(root/'target/release/examples/native_math_baseline'),str(source)],a.observe_seconds)
    label=f'analyze-{a.rounds or 11}'
    run(label,[str(root/'target/release/egg_layout'),'analyze','--source',str(source),'--output',str(out/label)]+(['--rounds',str(a.rounds)] if a.rounds is not None else []),a.observe_seconds)
