"""Fixed selected plans: legacy in-runner keys vs explicit SK and SK+MK.

All lanes use the same Cell observer and GPU executable. Rebind only checks
existing query shapes/wiring; no optimizer, training or candidate search runs.
"""
import json
from pathlib import Path
import subprocess
import sys
import program_modes as modes

def evaluate(result):
    lanes=result['lanes'];base=lanes['legacy']
    def services(lane):
        return (lane.get('cell_query_rules'),lane.get('cell_query_bindings'),lane.get('skipped_saturations'),
                (lane.get('joint') or {}).get('stats'),
                [(m['live_roots'],m['live_bindings']) for m in (lane.get('shared_markers') or [])])
    result['services_match']=bool(base.get('cell_assistance') and base.get('skipped_saturations',0)>0 and
        all(l.get('cell_assistance') and services(l)==services(base) for l in lanes.values()))
    result['no_observed_regression']=all(l.get('ratio_to_legacy') is not None and l['ratio_to_legacy']<=1 for l in lanes.values())
    result['ready_to_merge']=result['graph_passed'] and result['services_match'] and result['no_observed_regression'] and result['extraction_passed'] is True
    return result

def run(a,repo):
    resolve=lambda p:p if p.is_absolute() else repo/p
    out=resolve(a.output);worker=resolve(a.worker);layout=resolve(a.layout)
    if out.exists():raise ValueError('use a new explicit-key comparison directory')
    sources={'legacy':resolve(a.source),'explicit-sk':resolve(a.sk),'explicit-markers':resolve(a.marked)}
    binary=repo/'target/analyzer/release/cell-assisted-egglog'
    compiler=repo/'target/analyzer/release/examples/scheduled_spine'
    build=[['cargo','build','--offline','--release','--target-dir','target/analyzer','--example','scheduled_spine','--example','tiger_output_check'],
           ['cargo','build','--offline','--release','--manifest-path','cell-assisted-egglog/Cargo.toml','--target-dir','target/analyzer','--bin','cell-assisted-egglog']]
    if a.dry_run:
        print('Fixed-layout legacy/SK/SK+MK Cell comparison; same executable/worker, exact public graphs and Tiger; no solver.')
        return 0
    if not a.no_build:
        for cmd in build:subprocess.run(cmd,cwd=repo,check=True)
    if not worker.is_file():raise ValueError('missing verified GPU worker')
    out.mkdir(parents=True);lanes={};graphs={}
    for name,source in sources.items():
        rebound=out/(name+'.layout.json')
        with (out/(name+'.compile.log')).open('w') as log:
            subprocess.run([str(compiler),str(source),str(rebound),'--joint','--rebind',str(layout)],cwd=repo,stdout=log,stderr=subprocess.STDOUT,check=True)
        chosen=json.loads(layout.read_text());validated=json.loads(rebound.read_text())
        def wiring(file):
            rules=json.loads(json.dumps(file['rules']))
            for rule in rules:
                for slot in rule['plan']['shape']['slots']:slot[0]=''
            return rules
        if wiring(chosen)!=wiring(validated):raise ValueError('rebind changed selected rules/layouts')
        native='cell-stable-keys' if name=='legacy' else 'cell-stable-keys,cell-stable-freeze'
        if name=='explicit-markers' and a.marker_maintenance=='native':native+=',cell-spine-complete'
        options={'cell-assistance':'on','setup-policy':'legacy-stable' if name=='legacy' else 'source',
                 'boundary-programs':rebound,'export-extraction':'on'}
        dest,r,reused=modes.lane(repo,binary,source,'gpu',worker,None,repo/'out/program-modes/cache',False,native,options)
        (out/name).symlink_to(dest,target_is_directory=True)
        rt=r.get('runtime',{});timing=rt.get('runtime',{})
        lanes[name]=dict(directory=str(dest),source=str(source),source_sha256=modes.sha(source),reused=reused,
            complete=r.get('complete',False),snapshot_complete=r.get('snapshot_complete',False),
            run_seconds=timing.get('run_seconds'),setup_seconds=r.get('setup',{}).get('setup_seconds'),
            gpu_setup_seconds=r.get('gpu_setup_seconds'),cell_assistance=r.get('cell_assistance'),
            source_rewritten_by_runner=r.get('source_rewritten_by_runner'),
            cell_query_rules=timing.get('cell_query_rules'),cell_query_bindings=timing.get('cell_query_bindings'),
            skipped_saturations=timing.get('skipped_saturations'),
            shared_markers=rt.get('saturation_diagnostics',{}).get('shared_markers'),
            joint=rt.get('saturation_diagnostics',{}).get('joint'))
        if r.get('complete') and r.get('snapshot_complete'):graphs[name]=dest/'graph.json'
        modes.write(out/'progress.json',lanes)
        if not r.get('complete'):raise ValueError('failed '+name+'; see '+str(dest/'run.log'))
    checks={}
    def compare(left,right):
        # Never retain a previous lane's large Python graph during inference.
        l=json.loads(graphs[left].read_text());r=json.loads(graphs[right].read_text())
        return modes.compare(l,r,set(l['tables']))
    for left,right in [('legacy','explicit-sk'),('explicit-sk','explicit-markers')]:
        checks[left+'->'+right]=compare(left,right) if left in graphs and right in graphs else dict(status='inconclusive')
    graph_ok=all(c.get('status')=='equal' for c in checks.values())
    rt=lanes['legacy']['run_seconds']
    for name,lane in lanes.items():lane['ratio_to_legacy']=lane['run_seconds']/rt if rt and lane['run_seconds'] else None
    result=dict(schema='explicit-key-cell-bench/v1',scope='same-binary actual GPU runtime with identical selected layouts and checked Cell services; no solver',
        source_head=modes.git(repo,'rev-parse','HEAD'),dirty_files=modes.git(repo,'status','--short'),
        binary_sha256=modes.sha(binary),worker_sha256=modes.sha(worker),selected_layout_sha256=modes.sha(layout),
        lanes=lanes,comparisons=checks,graph_passed=graph_ok,extraction_lanes=list(lanes),marker_maintenance=a.marker_maintenance,
        no_observed_regression=all(l['ratio_to_legacy'] is not None and l['ratio_to_legacy']<=1 for l in lanes.values()),
        extraction_passed=None,ready_to_merge=False)
    modes.write(out/'report.json',result)
    if not a.no_extract:
        (out/'graphs').mkdir()
        for name,lane in lanes.items():(out/'graphs'/f'{name}.json').symlink_to(Path(lane['directory'])/'extraction-input.json')
        cmd=[sys.executable,str(repo/'experiments/raytrace_pass3_gpu/extract.py'),str(out)]
        with (out/'extraction.log').open('w') as log:p=subprocess.run(cmd,cwd=repo,stdout=log,stderr=subprocess.STDOUT)
        result['extraction_passed']=p.returncode==0
    evaluate(result)
    modes.write(out/'report.json',result)
    print(json.dumps({k:v['run_seconds'] for k,v in lanes.items()}));print('graphs:',graph_ok,'extraction:',result['extraction_passed'],'ready:',result['ready_to_merge'])
    return 0 if result['ready_to_merge'] else 1
