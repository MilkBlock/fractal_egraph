#!/usr/bin/env python3
"""Compare an accepted baseline; never rewrite it from a regression run."""
import argparse
import json
import math
from pathlib import Path

ENGINE_PHASES=tuple(f'{stage}_{part}_seconds' for stage in ('run','export') for part in ('search_apply','merge','rebuild'))
RUNTIME_PHASES=('read_seconds','setup_seconds','input_seconds','run_seconds','export_seconds','drop_seconds','seconds','process_seconds')+ENGINE_PHASES


def number(value,label):
    if isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value) or value<0:
        raise ValueError(f'invalid measurement: {label}')
    return value


def compare(base,current,time_tolerance=.30,size_tolerance=.05):
    if base.get('storage_cost','structural')!=current.get('storage_cost','structural'):
        raise ValueError('incompatible benchmark: storage cost policy')
    for key in ('schema','suite_sha256','copies','threads'):
        if key not in base or base[key]!=current.get(key):raise ValueError(f'incompatible benchmark: {key}')
    if base['schema']!='cell-ci-bench/v2' or not base['cases'] or base['cases'].keys()!=current['cases'].keys():
        raise ValueError('schema or benchmark case set changed')
    time_keys=('os','arch','cpu','python','rust','solver','runner_image')
    comparable=all(base['environment'].get(k) and base['environment'][k]==current['environment'].get(k) for k in time_keys)
    changes=[];failures=[]
    def metric(name,a,b,kind):
        a=number(a,name);b=number(b,name);delta=b-a
        tolerance=size_tolerance if kind=='size' else .15 if kind=='rss' else time_tolerance
        floor=0 if kind=='size' else 8*1024**2 if kind=='rss' else .1 if kind=='bake' else .002
        eligible=kind=='size' or comparable
        regression=eligible and delta>max(a*tolerance,floor)
        row=dict(metric=name,baseline=a,current=b,change=delta,ratio=b/a if a else None,
                 comparable=eligible,regression=regression,kind=kind)
        changes.append(row)
        if regression:failures.append(name)
    for name,old in base['cases'].items():
        new=current['cases'][name]
        if old['source_sha256']!=new['source_sha256'] or old['copies']!=new['copies']:
            raise ValueError(f'workload changed: {name}')
        if old['bytes'].keys()!=new['bytes'].keys() or old['modes'].keys()!=new['modes'].keys():
            raise ValueError(f'missing output or mode: {name}')
        for file,a in old['bytes'].items():metric(f'{name}/bytes/{file}',a,new['bytes'][file],'size')
        if old['bake']['capture_mode']!='fresh' or new['bake']['capture_mode']!='fresh':raise ValueError('fresh and reused Bake cannot be compared')
        if old['bake']['phase_seconds'].keys()!=new['bake']['phase_seconds'].keys():raise ValueError('Bake phase coverage changed')
        for phase,a in old['bake']['phase_seconds'].items():metric(f'{name}/bake/{phase}',a,new['bake']['phase_seconds'][phase],'bake')
        for phase in ('compiler_seconds','process_seconds'):metric(f'{name}/bake/{phase}',old['bake'][phase],new['bake'][phase],'bake')
        for mode,a in old['modes'].items():
            b=new['modes'][mode]
            for k in ('restored_enodes','restored_eclasses'):
                number(a['size'][k],k);number(b['size'][k],k)
                if a['size'][k]!=b['size'][k]:failures.append(f'{name}/{mode}/{k}: logical size changed')
            metric(f'{name}/{mode}/physical_rows',a['size']['physical_rows'],b['size']['physical_rows'],'size')
            metric(f'{name}/{mode}/rss',a['timing']['peak_rss_bytes'],b['timing']['peak_rss_bytes'],'rss')
            for phase in RUNTIME_PHASES:metric(f'{name}/{mode}/{phase}',a['timing'][phase],b['timing'][phase],'time')
    return dict(timings_comparable=comparable,failures=failures,changes=changes,
        policy=dict(time_tolerance=time_tolerance,size_tolerance=size_tolerance,
            runtime_floor_seconds=.002,bake_floor_seconds=.1,rss_tolerance=.15,rss_floor_bytes=8*1024**2),
        note=('Time/RSS gates are enabled for matching environments. ' if comparable else 'CPU/toolchain environment differs: time/RSS gates are disabled. ')+
             'Size gates remain active. Single-run alerts are not statistical significance claims.')


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--baseline',type=Path,required=True);p.add_argument('--current',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--summary',type=Path)
    p.add_argument('--accept-regression',action='store_true',help='Acknowledge measured regressions; never bypass malformed data checks')
    p.add_argument('--time-tolerance',type=float,default=.30);p.add_argument('--size-tolerance',type=float,default=.05)
    a=p.parse_args()
    if any(not math.isfinite(v) or v<0 for v in (a.time_tolerance,a.size_tolerance)):p.error('tolerances must be finite and nonnegative')
    current=json.loads(a.current.read_text())
    # Always validate current measurements, including the first baseline run.
    compare(current,current,a.time_tolerance,a.size_tolerance)
    if a.baseline.exists():result=compare(json.loads(a.baseline.read_text()),current,a.time_tolerance,a.size_tolerance)
    else:result=dict(failures=[],changes=[],status='no_baseline',note='First accepted default-branch run establishes the baseline; this run has no historical regression verdict.')
    result['accepted_regression']=bool(a.accept_regression and result['failures'])
    a.output.write_text(json.dumps(result,indent=2)+'\n')
    lines=['','## Regression check','',result.get('note','')]
    if result['failures']:lines+=['','Regressions:',*['- '+s for s in result['failures']]]
    if result['accepted_regression']:lines+=['','These regressions were explicitly accepted by the manual run.']
    if not result['failures']:lines+=['','No historical baseline yet.' if result.get('status')=='no_baseline' else 'No regression crossed the configured thresholds.']
    if a.summary:
        with a.summary.open('a') as f:f.write('\n'.join(lines)+'\n')
    print('\n'.join(lines))
    if result['failures'] and not a.accept_regression:raise SystemExit(1)


if __name__=='__main__':main()
