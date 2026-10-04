"""Separate-process, paired cold/parent-snapshot/fused-order native experiment."""
import argparse
import hashlib
import json
import os
import pathlib
import re
import statistics
import subprocess
import time

p=argparse.ArgumentParser()
p.add_argument('--analysis',type=pathlib.Path,required=True)
p.add_argument('--cell',type=int,default=1706)
p.add_argument('--output',type=pathlib.Path,required=True)
p.add_argument('--binary',type=pathlib.Path,default=pathlib.Path('target/release/examples/ripsat_graft'))
p.add_argument('--repeats',type=int,default=3)
p.add_argument('--ms',type=int,default=50)
p.add_argument('--finite-from',type=pathlib.Path,help='restrict both paths to cases equal in every snapshot comparison of a prior report')
a=p.parse_args()
assert a.repeats>0 and a.ms>0
out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
binary=str(a.binary.resolve());manifest=out/'workload.json'
subprocess.run([binary,'prepare',str(a.analysis.resolve()),str(a.cell),str(manifest)],check=True)
selection_basis=None
if a.finite_from:
    previous=json.loads(a.finite_from.read_text())
    if previous['workload_sha256'] != hashlib.sha256(manifest.read_bytes()).hexdigest():
        raise SystemExit('selection report was produced from a different workload')
    cs=[c for c in previous['comparisons'] if c['right']=='snapshot']
    if not cs:raise SystemExit('no snapshot comparisons in selection report')
    allowed=set.intersection(*[{(r['family'],r['case']) for r in c['cases'] if r['result']=='equal'} for c in cs])
    families=json.loads(manifest.read_text())
    for family in families:
        family['grafts']=[g for g in family['grafts'] if (family['id'],g['id']) in allowed]
    families=[f for f in families if f['grafts']]
    manifest.write_text(json.dumps(families,indent=2)+'\n')
    selection_basis=hashlib.sha256(a.finite_from.read_bytes()).hexdigest()
env={**os.environ,'RUST_MIN_STACK':'16777216'}
records=[];checks=[]
for repeat in range(a.repeats):
    order=['rebuild','snapshot','fused'] if repeat%2==0 else ['fused','snapshot','rebuild']
    for mode in order:
        report=out/f'{mode}-{repeat}.json';log=out/f'{mode}-{repeat}.log'
        begin=time.monotonic()
        with log.open('w') as f:
            subprocess.run(['/usr/bin/time','-l',binary,'run',str(manifest),mode,str(report),str(a.ms)],env=env,stdout=f,stderr=f,check=True)
        text=log.read_text()
        def field(label):
            m=re.search(r'(\d+)\s+'+label,text)
            return int(m[1]) if m else None
        records.append(dict(mode=mode,repeat=repeat,wall_seconds=time.monotonic()-begin,
                            max_rss_bytes=field('maximum resident set size'),peak_footprint_bytes=field('peak memory footprint')))
    for mode in ['snapshot','fused']:
        dest=out/f'compare-{mode}-{repeat}.json'
        subprocess.run([binary,'compare',str(out/f'rebuild-{repeat}.json'),str(out/f'{mode}-{repeat}.json'),str(dest)],check=True)
        c=json.loads(dest.read_text());c['repeat']=repeat;checks.append(c)
        if mode=='snapshot' and any(x['result']=='different' for x in c['cases']):
            raise SystemExit(f'SNAPSHOT MISMATCH: {dest}')

def reports(mode):return [json.loads((out/f'{mode}-{r}.json').read_text()) for r in range(a.repeats)]
metrics={}
for mode in ['rebuild','snapshot','fused']:
    runs=reports(mode);families=[]
    for i,first in enumerate(runs[0]['families']):
        fs=[r['families'][i] for r in runs];cases=[]
        for j,c in enumerate(first['cases']):
            cs=[f['cases'][j] for f in fs]
            cases.append(dict(id=c['id'],statuses=[c['status'] for c in cs],
              execution_seconds_median=statistics.median(c.get('execution_seconds',0) for c in cs),
              fork_seconds_median=statistics.median(c.get('fork_seconds',0) for c in cs),
              amortized_seconds_median=statistics.median(c.get('execution_seconds',0)+(f.get('parent_bake') or {}).get('preparation_seconds',0)/len(f['cases']) for c,f in zip(cs,fs)),
              parent_matches=[(c.get('parent_run') or {}).get('logical_matches',0) for c in cs],
              child_matches=[sum(s.get('logical_matches',0) for s in c.get('steps',[])) for c in cs],
              parent_unchanged=[c.get('parent_unchanged') for c in cs],
              shared_backings_before=[c.get('storage_before') for c in cs]))
        families.append(dict(family=first['family'],cases=cases,
          setup_seconds_median=statistics.median(f['setup_seconds'] for f in fs),
          bake_seconds_median=statistics.median((f.get('parent_bake') or {}).get('preparation_seconds',0) for f in fs)))
    rr=[r for r in records if r['mode']==mode]
    metrics[mode]=dict(families=families,wall_seconds_median=statistics.median(r['wall_seconds'] for r in rr),
       max_rss_bytes_median=statistics.median(r['max_rss_bytes'] for r in rr),
       peak_footprint_bytes_median=statistics.median(r['peak_footprint_bytes'] for r in rr))
summary=dict(scope='Actual native engine in standalone differential harness; not automatic analyze/runtime reuse',
    workload_sha256=hashlib.sha256(manifest.read_bytes()).hexdigest(),binary_sha256=hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest(),
    source_sha256=hashlib.sha256((a.analysis/'source.egg').read_bytes()).hexdigest(),
    selection='All distinct Get operand terms from the real source initialization; one previously observed getter parent plus explicit semantic-contract fixtures',
    cell=a.cell,finite_selection_report_sha256=selection_basis,repeats=a.repeats,limits=dict(milliseconds_per_closure=a.ms,facts=200000),
    environment={'RUST_MIN_STACK':'16777216'},runs=records,comparisons=checks,metrics=metrics,
    limitations=['Incomplete runs are inconclusive, never equivalent.',
       'Rebuild saturates the parent before grafting; fused intentionally changes that order.',
       'Execution timings exclude serialization, comparison and manifest parsing. Wall/RSS include the full process.',
       'The snapshot parent replaces the unused scratch engine; retaining an extra compiled base would increase its memory footprint.',
       'Snapshot totals include a one-time parent bake; amortized per-graft costs divide that bake over this finite batch.',
       'Shared-backing byte counts exclude owned rows and all indexes/metadata; they are not total memory or compression ratios.',
       'No terminal state cache. A successful differential test is not a proof for all bindings or rule environments.'])
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
for c in checks:print(c['right'],c['repeat'],{k:sum(x['result']==k for x in c['cases']) for k in ['equal','different','inconclusive']})
