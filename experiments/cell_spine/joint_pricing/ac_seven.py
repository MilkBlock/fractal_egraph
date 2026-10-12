#!/usr/bin/env python3
"""Verify finite-vs-saturated seven-input AC, then audit the normal Bake path.

This is only a fixture/probe. There is no AC recognition or special compiler.
The external process guard stops the experiment, never prunes Bake candidates.
"""
import argparse
from collections import Counter
import json
import os
from pathlib import Path
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path[:0]=[str(ROOT),str(ROOT/'tools/bench')]
from math_virtual import measure
from math_graph_equal import verify
from tools.cell_spine import compiler as c
from tools.cell_spine.terms import actions

SOURCE=ROOT/'experiments/cell_spine/fixtures/ac_seven.egg'
RUNNER=ROOT/'target/release/examples/native_math_baseline'


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--reuse-capture',type=Path,help='reuse a prior guarded analysis of the identical saturated fixture')
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    os.environ['RAYON_NUM_THREADS']='1'
    raw=SOURCE.read_text()
    if raw.count('(run add-ac 7)')!=1:raise ValueError('finite schedule changed')
    sat=raw.replace('(run add-ac 7)','(run-schedule (saturate add-ac))')
    saturated=out/'saturated-source.egg';saturated.write_text(sat)
    data=c.ast(SOURCE,out/'input.json')
    sat_data=c.ast(saturated,out/'sat-input.json')
    env='\n'.join(c.commands(data,{'declaration','rule'}))+'\n'
    (out/'environment.egg').write_text(env)
    (out/'input.egg').write_text('\n'.join(actions([a['action'] for a in data['actions']]))+'\n')
    evidence={}
    for mode,d in [('finite7',data),('saturated',sat_data)]:
        m=dict(environment=str(out/'environment.egg'),input=str(out/'input.egg'),constructors=sorted(data['signatures']),
               run='\n'.join(c.commands(d,{'schedule','check'})))
        (out/f'{mode}.json').write_text(json.dumps(m,indent=2)+'\n')
        v=measure([RUNNER,'bench-probes',out/f'{mode}.json',out/f'{mode}-graph.json'],out/f'verify-{mode}')
        t=measure([RUNNER,'bench',out/f'{mode}.json'],out/f'time-{mode}')
        evidence[mode]=dict(verification=v['result']['verification'],timing=t)
    equality=verify(out/'finite7-graph.json',out/'saturated-graph.json',['Math'])
    if (equality['enodes'],equality['eclasses'])!=(1939,127):raise ValueError('seven-input closure changed')
    report=dict(source=str(SOURCE.relative_to(ROOT)),source_sha256=c.digest(SOURCE),runner_sha256=c.digest(RUNNER),
        equality=equality,native=evidence,original_schedule_preserved=True,
        compiler_admission=dict(finite7=c.compatible_program(data),saturated=c.compatible_program(sat_data)),
        scope='Actual native current egglog, finite7 and a separate saturation fixture; no special AC backend')
    capture=a.reuse_capture.resolve() if a.reuse_capture else out
    if a.reuse_capture:
        if (capture/'saturated-source.egg').read_text()!=sat:raise ValueError('capture source changed')
    else:
        try:
            measured=measure([ROOT/'target/release/egg_layout','analyze','--source',saturated,'--output',capture/'analysis'],capture/'capture',timeout=60)
            (capture/'capture.json').write_text(json.dumps(measured,indent=2)+'\n')
        except RuntimeError as error:
            (capture/'capture-failed.json').write_text(json.dumps(dict(error=str(error)),indent=2)+'\n')
    failure=capture/'capture-failed.json'
    report['analysis']=dict(path=str(capture/'analysis'),completed=(capture/'analysis/analysis.json').exists(),
        measurement=(json.loads((capture/'capture.json').read_text()) if (capture/'capture.json').exists() else None),
        failure=json.loads(failure.read_text()) if failure.exists() else None,
        external_experiment_guard=dict(seconds=60,process_group_rss_bytes=4*1024**3),
        candidate_pruning=False,solver_reached=False)
    partial=Counter()
    for file in (capture/'analysis/saturated-rule-composition/cells').glob('*/ripen.json'):
        r=json.loads(file.read_text());partial[r['ripen']['state']]+=1
    report['analysis']['partial_cell_states']=dict(partial)
    if report['analysis']['completed']:
        command=[sys.executable,'-m','tools.cell_spine.compiler','--source',str(saturated),
            '--analysis-dir',str(capture/'analysis'),'--output',str(out/'compiled'),'--skip-build']
        try:
            report['compilation']=measure(command,out/'compile',timeout=60)
            report['compiled']=json.loads((out/'compiled/report.json').read_text())['status']=='compiled'
            report['analysis']['solver_reached']=True
        except RuntimeError as error:
            report['compiled']=False;report['compilation_error']=str(error)
            report['analysis']['solver_reached']=(out/'compiled/work/pricing').exists()
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(dict(equality=equality,analysis=report['analysis']),indent=2))


if __name__=='__main__':main()
