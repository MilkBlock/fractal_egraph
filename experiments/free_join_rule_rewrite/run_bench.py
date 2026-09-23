#!/usr/bin/env python3
"""Repeat the actual egglog 3.0 math workload with and without decomposition."""
import argparse, json, re, statistics, subprocess, time
from pathlib import Path

def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--runs',type=int,default=9); ap.add_argument('-o','--output',type=Path,default=Path('results/timing-summary.json')); a=ap.parse_args()
    root=Path(__file__).resolve().parent; binary=root/'vendor/egglog-3.0.0-src/target/release/egglog'; program=root/'math-microbenchmark.egg'
    rows=[]
    for mode,args in [('baseline',[]),('no_decomp',['--no-decomp'])]:
        for run in range(1,a.runs+1):
            start=time.perf_counter(); p=subprocess.run([str(binary),*args,str(program)],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,check=True); wall=time.perf_counter()-start
            m=re.search(r'Ruleset : search ([0-9.]+)s, merge ([0-9.]+)s, rebuild ([0-9.]+)s',p.stdout)
            if not m: raise RuntimeError('print-stats output missing')
            rows.append({'mode':mode,'run':run,'wall_s':wall,'search_s':float(m.group(1)),'merge_s':float(m.group(2)),'rebuild_s':float(m.group(3))})
    medians={}
    for mode in ('baseline','no_decomp'):
        xs=[r for r in rows if r['mode']==mode and r['run']>1]
        medians[mode]={k:statistics.median(r[k] for r in xs) for k in ('wall_s','search_s','merge_s','rebuild_s')}
    out={'runtime':'official egglog v3.0.0 source, release build, one thread','warmup_policy':'first run per mode excluded','runs':rows,'medians':medians}
    a.output.parent.mkdir(parents=True,exist_ok=True); a.output.write_text(json.dumps(out,indent=2)+'\n')
if __name__=='__main__': main()
