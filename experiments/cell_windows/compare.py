#!/usr/bin/env python3
"""One native analyze run per mode. No averages, graph rendering or all-path search."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

ROOT=Path(__file__).resolve().parents[2]


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source',type=Path,default=ROOT/'experiments/cell_windows/chain.egg')
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    results=[]
    for mode,enabled in [('rooted','0'),('windows','1')]:
        env=dict(os.environ,EGG_LAYOUT_CELL_WINDOWS=enabled,EGG_LAYOUT_GRAPHS='0',
            EGG_LAYOUT_ANALYSIS_THREADS='1',EGG_LAYOUT_QUERY_THREADS='1',RAYON_NUM_THREADS='1')
        tick=time.perf_counter()
        with (out/f'{mode}.log').open('w') as log:
            subprocess.run([ROOT/'target/release/egg_layout','analyze','--source',a.source.resolve(),
                '--output',out/mode],cwd=ROOT,env=env,check=True,stdout=log,stderr=subprocess.STDOUT)
        seconds=time.perf_counter()-tick
        queue=json.loads((out/mode/'analysis.json').read_text())['saturated_rule_composition']
        graph=queue['cell_egraph'];metrics=graph['reuse_value']['cells']
        result=dict(mode=mode,seconds=seconds,entries=len(queue['jobs']),states=queue['counts'],
            certified_cell_kinds=len(metrics),window_readings=len(graph['windows']),cells=metrics,
            preparation=queue['preparation_cache'])
        results.append(result)
        print({k:v for k,v in result.items() if k not in ('cells','preparation')},flush=True)
    (out/'comparison.json').write_text(json.dumps(dict(source=str(a.source.resolve()),runs=results,
        scope='one run per mode; native analyzer; timings include cold-start and filesystem noise'),indent=2)+'\n')


if __name__=='__main__':main()
