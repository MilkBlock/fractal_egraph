"""macOS release capture profiling: phase times, process RSS and native stacks.
Build release binaries first. No trace JSON is written by this runner.
"""
import argparse,json,os,signal,subprocess,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def main():
    p=argparse.ArgumentParser();p.add_argument('--rounds',type=int,default=6);p.add_argument('--timeout',type=float,default=120);p.add_argument('--max-rss-mib',type=int,default=8192);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    env=dict(os.environ,CARGO_INCREMENTAL='0')
    command=[str(ROOT/'target/release/egg_layout'),'analyze','--recapture-tier0','--source','egglog/tests/math-microbenchmark.egg','--rounds',str(a.rounds),'--output',str(out/'run')]
    start=time.monotonic();peaks={};samplers=[];sampled=set();timed_out=False;stop_reason=None;peak_tree_rss=0
    with (out/'capture.log').open('w') as log:
        proc=subprocess.Popen(command,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,env=env,start_new_session=True)
        while proc.poll() is None:
            lines=subprocess.check_output(['ps','-axo','pid=,ppid=,rss=,pcpu=,comm='],text=True).splitlines();rows={}
            for line in lines:
                fields=line.strip().split(None,4)
                if len(fields)==5:
                    pid,ppid,rss,cpu,name=fields;rows[int(pid)]=(int(ppid),int(rss),float(cpu),name)
            tree={proc.pid}
            while True:
                new={pid for pid,r in rows.items() if r[0] in tree}
                if new<=tree:break
                tree|=new
            for pid in tree:
                if pid not in rows:continue
                _,rss,cpu,name=rows[pid];label=Path(name).name
                record=peaks.setdefault(pid,{'pid':pid,'process':label,'rss_kib':0,'cpu_percent':0})
                record['rss_kib']=max(record['rss_kib'],rss);record['cpu_percent']=max(record['cpu_percent'],cpu)
                if label in ['combine_profile','tier1_export','tier1_interface_snapshot'] and pid not in sampled:
                    sampled.add(pid)
                    with (out/f'{label}-{pid}.sample.log').open('w') as f:
                        samplers.append(subprocess.Popen(['sample',str(pid),'2','1','-file',str(out/f'{label}-{pid}.sample.txt')],stdout=f,stderr=subprocess.STDOUT))
            tree_rss=sum(rows[pid][1] for pid in tree if pid in rows);peak_tree_rss=max(peak_tree_rss,tree_rss)
            if time.monotonic()-start>a.timeout or tree_rss>a.max_rss_mib*1024:
                timed_out=time.monotonic()-start>a.timeout
                stop_reason='timeout' if timed_out else 'process-tree RSS limit'
                os.killpg(proc.pid,signal.SIGTERM)
                try:proc.wait(timeout=5)
                except subprocess.TimeoutExpired:os.killpg(proc.pid,signal.SIGKILL)
                break
            time.sleep(.15)
        proc.wait()
    for sample in samplers:sample.wait()
    result={'rounds':a.rounds,'wall_seconds':time.monotonic()-start,'exit_code':proc.returncode,'timed_out':timed_out,'stop_reason':stop_reason,'peak_tree_rss_kib':peak_tree_rss,'process_peaks':list(peaks.values()),'measurement':'release; compiler work excluded by prebuild except cached cargo check; native sampler and ps polling add overhead'}
    marker=out/'run/run.json'
    if marker.exists():
        result['run']=json.loads(marker.read_text())
        if stop_reason and result['run'].get('status')!='complete':
            result['run'].update(status='interrupted',stop_reason=stop_reason)
            marker.write_text(json.dumps(result['run'],indent=2)+'\n')
    (out/'profile.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
if __name__=='__main__':main()
