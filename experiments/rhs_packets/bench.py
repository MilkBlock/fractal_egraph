"""Single paired full-runtime measurement. Compilation excluded, no averaging."""
import argparse,copy,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
from experiments.resident_restoration.check import run
from test_generic_cells import core_for
from eggfuse.analyzer.regions import from_core
from eggfuse.engine.regions import execution_summary
p=argparse.ArgumentParser();p.add_argument('--build',required=True,type=Path);p.add_argument('--output',required=True,type=Path);p.add_argument('--bindings',type=int,default=32768);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
src=a.output/'nested.egg';src.write_text('''(datatype E (N i64) (P E i64) (Q E) (Pair E E)) (relation Seed (E i64)) (ruleset s)
(rule ((Seed x n)) ((Q (P x n)) (Pair (P x n) x)) :ruleset s)
'''+''.join(f'(Seed (N {i}) {i})\n' for i in range(a.bindings))+'(run s 1)')
c=core_for(src,a.build);plan=from_core(c);runs={};results={}
for on in [False,True]:
    runs[on]=r=run(c,plan,a.build,a.output/str(on),parallel_rhs_packets=on,audit=False,capacity=8*a.bindings,match_capacity=4*a.bindings)
    results[str(on)]=dict(inference_seconds=r['programs'][0]['inference_seconds'],work=execution_summary(r),worker=r['worker_provenance']['binary_sha256'])
x,y=[r['programs'][0]['instances'][0] for r in runs.values()]
for k in ('rows','parents','fact_parents','meta','history','sets','watermarks'):assert x[k]==y[k],k
assert results['False']['worker']==results['True']['worker']
results.update(bindings=a.bindings,full_state_and_history_equal=True,speedup=results['False']['inference_seconds']/results['True']['inference_seconds'],scope='actual resident engine, synthetic nested RHS workload; not Pass 3')
(a.output/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(results,indent=2))
