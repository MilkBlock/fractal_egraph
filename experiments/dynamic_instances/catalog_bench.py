#!/usr/bin/env python3
"""Actual frozen runtime: local exact catalog versus imported analysis catalog."""
import json, os, pathlib, subprocess, sys, time
root=pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0,str(root/'tools/bench'))
from math_graph_equal import verify
out=root/sys.argv[1]; catalog=(root/sys.argv[2]).resolve()
out.mkdir(parents=True,exist_ok=False)
results=[]
for label in ['local','analysis']:
    env=dict(os.environ,EGG_LAYOUT_CELL_MACHINE='1',EGG_LAYOUT_DYNAMIC_INSTANCES='1',EGG_LAYOUT_AUDIT_VERIFY='1',EGG_LAYOUT_RUNTIME_SUMMARY='1')
    env.pop('EGG_LAYOUT_CELL_CATALOG',None)
    if label=='analysis': env['EGG_LAYOUT_CELL_CATALOG']=str(catalog)
    cmd=[str(root/'target/release/examples/adaptive_math'),'frozen',str(root/'experiments/math_mini3/source.egg'),str(out/label),str(root/'experiments/math_mini3/adaptive_runtime/bake_lib_round2.json')]
    start=time.perf_counter()
    with (out/(label+'.log')).open('w') as log: subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=120)
    report=json.loads((out/label/'report.json').read_text()); eq=json.loads((out/label/'cell-equivalences.json').read_text())
    results.append(dict(variant=label,wall_seconds=time.perf_counter()-start,setup_seconds=report['setup_seconds'],execution_seconds=report['execution_seconds'],graph=report['final'],imported_states=eq['imported_states'],skipped_states=eq['skipped_states'],matched_analysis_rules=sum(e['analysis_state'] is not None for e in eq['entries']),entries=eq['entries'],runtime_instances=report['dynamic_instances']['instances']))
equality=verify(out/'local/verification-graph.json',out/'analysis/verification-graph.json')
(out/'results.json').write_text(json.dumps(dict(runs=results,equality=equality),indent=2)+'\n')
print(json.dumps(dict(runs=results,equality=equality),indent=2))
