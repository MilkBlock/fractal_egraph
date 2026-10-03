#!/usr/bin/env python3
"""Math3: discovery admission and query compilation are measured separately."""
import json,os,pathlib,subprocess,sys,time
root=pathlib.Path(__file__).resolve().parents[2];out=root/sys.argv[1]
out.mkdir(parents=True,exist_ok=False)
sys.path.insert(0,str(root/'tools/bench'))
from math_graph_equal import verify
results=[]
for label,mode in [('layers','layers'),('events','events')]:
    env=dict(os.environ,EGG_LAYOUT_GRAPHS='0',EGG_LAYOUT_COMPOSITION_DISCOVERY=mode)
    cmd=[str(root/'target/release/egg_layout'),'analyze','--source',str(root/'egglog/tests/math-microbenchmark.egg'),'--rounds','3','--output',str(out/label)]
    start=time.perf_counter()
    with (out/(label+'.log')).open('w') as log:subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
    r=json.loads((out/label/'analysis.json').read_text());cs=r['saturated_rule_composition']['cs']
    results.append(dict(variant=label,seconds=time.perf_counter()-start,applications=r['summary']['applications'],states=len(cs['units']),connections=len(cs['compositions']),cross_rule_connections=sum(any(cs['compositions'][cs['units'][p]['source_composition']]['rule']!=c['rule'] for p in c['parts'] if cs['units'][p]['source_composition'] is not None) for c in cs['compositions']) if mode=='events' else None,coarse=sum(c['kind']=='ObservedCoarseApply' for c in cs['compositions']),smooth=sum(c['kind']=='ObservedSmoothApply' for c in cs['compositions']),confirmations=cs['confirmations_without_new_effect'],legacy_layers=len(r['layers']['coarse_layers']),ripen_counts=r['saturated_rule_composition']['counts']))
for label,enabled in [('compiled-only','0'),('observed','1')]:
    env=dict(os.environ,EGG_LAYOUT_CELL_MACHINE='1',EGG_LAYOUT_OBSERVED_GRAFTS=enabled,EGG_LAYOUT_AUDIT_VERIFY='1',EGG_LAYOUT_RUNTIME_SUMMARY='1')
    cmd=[str(root/'target/release/examples/adaptive_math'),'frozen',str(root/'experiments/math_mini3/source.egg'),str(out/label),str(root/'experiments/math_mini3/adaptive_runtime/bake_lib_round2.json')]
    start=time.perf_counter()
    with (out/(label+'.log')).open('w') as log:subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=120)
    r=json.loads((out/label/'report.json').read_text());d=r['dynamic_instances']
    results.append(dict(variant=label,seconds=time.perf_counter()-start,execution_seconds=r['execution_seconds'],state_seconds=r['state_transition_seconds'],observed_grafts=d['observed_graft_count'],binding_patterns=d['graft_binding_pattern_count'],compiled_links=d['compiled_link_count'],state_revisions=d['state_revisions'],source_matches_without_cell_receipts=r['source_rule_matches_without_cell_receipts'],graph=r['final']))
equal=verify(out/'compiled-only/verification-graph.json',out/'observed/verification-graph.json')
(out/'results.json').write_text(json.dumps(dict(runs=results,equality=equal),indent=2)+'\n')
print(json.dumps(dict(runs=results,equality=equal),indent=2))
