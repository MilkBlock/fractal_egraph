"""Source-level spine marker lane. No GPU/Cell-sat credit is implied."""
import json
import os
from pathlib import Path
import subprocess
import sys


def summarize(program,run):
    cfg=json.loads((program/'markers.json').read_text())
    ast=json.loads((program/'source-ast.json').read_text())
    report=json.loads((run/'report.json').read_text())
    rules={r['ordinal']:r for r in ast['rules']}
    counts=report['run_report']['num_matches_per_rule'];rows=[]
    for view in cfg['markers']:
        for o in view['occurrences']:
            r=rules[o['rule']];count=counts.get(r['name'])
            rows.append(dict(marker=view['name'],analyze_states=view['cells'],rule=r['name'],ruleset=r['ruleset'],
                reported_in_run=count is not None,logical_matches=count,marker_rows=report['marker_rows'].get(view['name'],0)))
    unique={r['rule']:r['logical_matches'] for r in rows}
    quality=run/'extraction/quality.json'
    result=dict(schema='source-marker-ledger/v1',rows=rows,successful_consumer_bindings=sum(n or 0 for n in unique.values()),
        full_graph_passed=bool(report.get('verification')),extraction_passed=json.loads(quality.read_text())['passed'] if quality.exists() else None,
        scope='shared structural view work; not sat-skip, not live Cell hit rate; unreported consumers remain null')
    (run/'source-marker-ledger.json').write_text(json.dumps(result,indent=2)+'\n')
    return result


def run(a,repo,sha):
    resolve=lambda p:p if p.is_absolute() else repo/p
    source=resolve(a.snapshot);control=resolve(a.reuse_controls);out=resolve(a.output)
    if out.exists():raise ValueError('use a new output directory')
    workflow=json.loads((control/'workflow.json').read_text())
    if workflow.get('returncode')!=0:raise ValueError('control workflow did not pass')
    if workflow.get('snapshot_sha256',workflow.get('source_sha256'))!=sha(source) or workflow.get('policy')!='stable':raise ValueError('native control input/policy differs')
    python=a.solver_python
    if python is None:
        options=[repo/'out/cell-spine-venv/bin/python',Path('/Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python')]
        python=next((p for p in options if p.is_file()),Path(sys.executable))
    env=dict(os.environ,RAYON_NUM_THREADS='1',CELL_SEARCH_COUNTERS='1' if a.profile else '0')
    program=resolve(a.compiled) if a.compiled else out/'program'
    commands=[]
    if not a.no_build:
        commands.append(['cargo','build','--offline','--release','--example','lhs_ast','--example','spine_pass3_bench'])
    if not a.compiled:
        commands.append([python,'-m','tools.cell_spine.pass3_markers','--source',source,'--analysis',resolve(a.analysis),'--output',program])
    candidate=source if a.variant=='original' else program/(a.variant+'.egg')
    commands.append([repo/'target/release/examples/spine_pass3_bench',source,candidate,program/'markers.json',control,out/'run'])
    if not a.no_extract:
        commands.append([sys.executable,repo/'experiments/raytrace_pass3_gpu/extract.py',out/'run','--tiger',resolve(a.tiger)])
    if a.dry_run:
        for cmd in commands:print(' '.join(map(str,cmd)))
        return 0
    if a.compiled:
        proof=json.loads((program/'markers.json').read_text())
        if proof['source_sha256']!=sha(source):raise ValueError('compiled markers use a different source')
        analysis=resolve(a.analysis)
        files=[analysis/'catalog/catalog.json']+sorted((analysis/'catalog/states').glob('*.json'))
        if proof.get('analysis_sha256')!={str(p.relative_to(analysis)):sha(p) for p in files}:
            raise ValueError('compiled marker library fingerprint differs or is missing; regenerate source')
        if a.variant!='original' and proof['variants'][a.variant]['sha256']!=sha(candidate):raise ValueError('generated source changed')
    out.mkdir(parents=True);status=0
    with (out/'run.log').open('w') as log:
        for cmd in commands:
            cmd=list(map(str,cmd));print(' '.join(cmd),flush=True)
            rc=subprocess.run(cmd,cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT).returncode
            status=status or rc
            if rc and not (out/'run/graphs/markers.json').is_file():break
    if (out/'run').exists():
        p=dict(mode='marker-pass3',variant=a.variant,policy='stable',profile=a.profile,source_sha256=sha(source),
               generated_sha256=sha(candidate),control=str(control),returncode=status,
               binary_sha256=sha(repo/'target/release/examples/spine_pass3_bench'),commands=[[str(x) for x in c] for c in commands])
        (out/'run/workflow.json').write_text(json.dumps(p,indent=2)+'\n')
        if (out/'run/report.json').exists():
            summary=summarize(program,out/'run')
            print(json.dumps({k:summary[k] for k in ['full_graph_passed','extraction_passed','successful_consumer_bindings']}))
    print(out/'run/report.json');return status
