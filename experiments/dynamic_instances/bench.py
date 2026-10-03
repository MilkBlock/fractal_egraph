#!/usr/bin/env python3
"""Single-screen same-library dynamic instance runtime ablation."""
import json, os, pathlib, subprocess, sys, time
root=pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0,str(root/'tools/bench'))
from math_graph_equal import verify
out=root/sys.argv[1]
out.mkdir(parents=True,exist_ok=False)
results=[]
for label,enabled in [('legacy','0'),('dynamic','1')]:
    env=dict(os.environ,EGG_LAYOUT_CELL_MACHINE='1',EGG_LAYOUT_DYNAMIC_INSTANCES=enabled,EGG_LAYOUT_AUDIT_VERIFY='1',EGG_LAYOUT_RUNTIME_SUMMARY='1')
    command=[str(root/'target/release/examples/adaptive_math'),'frozen',str(root/'experiments/math_mini3/source.egg'),str(out/label),str(root/'experiments/math_mini3/adaptive_runtime/bake_lib_round2.json')]
    start=time.perf_counter()
    with (out/(label+'.log')).open('w') as log:
        subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=120)
    report=json.loads((out/label/'report.json').read_text())
    results.append({'variant':label,'wall_seconds':time.perf_counter()-start,'execution_seconds':report['execution_seconds'],'packing_seconds':report['packing_seconds'],'state_transition_seconds':report.get('state_transition_seconds',0),'graph':report['final'],'dynamic_instances':report['dynamic_instances']})
equality=verify(out/'legacy/verification-graph.json',out/'dynamic/verification-graph.json')
(out/'results.json').write_text(json.dumps({'runs':results,'equality':equality},indent=2)+'\n')
print(json.dumps({'runs':results,'equality':equality},indent=2))
