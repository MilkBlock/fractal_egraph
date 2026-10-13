#!/usr/bin/env python3
"""Portable, single-run native size/phase benchmarks for GitHub Actions.

Bake one seed; replicate its constructor-only String-renaming symmetry for a
measurable runtime workload. Counts and timings run in separate processes. No
SVG, graph dumps, raw tracing, repeated timing averages, or copied local catalogs.
"""
import argparse
import copy
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT))
from tools.cell_spine import compiler as c,markers
from tools.cell_spine import solver
from tools.cell_spine.terms import actions,walk

PHASES=('read_seconds','setup_seconds','input_seconds','run_seconds','export_seconds','drop_seconds','seconds')
ENGINE_PHASES=tuple(f'{stage}_{part}_seconds' for stage in ('run','export') for part in ('search_apply','merge','rebuild'))
MODES=('native','raw_ingress','input_spine','input_spine_export')


def measure(command,prefix):
    """wait4 returns this child's RSS, not a cumulative prior-child maximum."""
    if platform.system() not in ('Linux','Darwin'):raise ValueError('benchmark supports Linux and macOS')
    start=time.perf_counter()
    with prefix.with_suffix('.stdout').open('w') as stdout,prefix.with_suffix('.stderr').open('w') as stderr:
        proc=subprocess.Popen(list(map(str,command)),cwd=ROOT,stdout=stdout,stderr=stderr)
        _,status,usage=os.wait4(proc.pid,0)
        proc.returncode=os.waitstatus_to_exitcode(status)
    wall=time.perf_counter()-start
    if proc.returncode:
        raise RuntimeError(f'{command[0]} exited {proc.returncode}; see {prefix.with_suffix(".stderr")}')
    return dict(process_seconds=wall,peak_rss_bytes=int(usage.ru_maxrss)*(1024 if platform.system()=='Linux' else 1))


def read_result(prefix):return json.loads(prefix.with_suffix('.stdout').read_text().splitlines()[-1])


def rule_terms(rule):
    for f in rule['body']:
        for e in f['args'] if f['kind']=='eq' else [f['expr']]:yield from walk(e)
    for a in rule['head']:
        for e in [a['expr']] if a['kind'] in ('expr','let') else a['args']:yield from walk(e)
        if a['kind']=='set':yield from walk(a['value'])


def check_renaming(data,generated_rules):
    # String renaming must be an automorphism of the rule environment. Never
    # change fixed literal guards or String primitives just to get a big bench.
    for r in data['rules']:
        for e in rule_terms(r):
            if e['kind']=='call' and e['head'] not in data['signatures']:
                raise ValueError('runtime replication requires constructor-only source rules')
    for r in data['rules']+generated_rules:
        if any(e['kind']=='literal' and e['sort']=='String' for e in rule_terms(r)):
            raise ValueError('fixed String rule literals prevent safe input replication')


def replicas(acts,count):
    result=[]
    for i in range(count):
        items=copy.deepcopy(acts)
        rename={a['name']:f'ci_{i}_{a["name"]}' for a in items if a['kind']=='let'}
        for a in items:
            if a['kind']=='let':a['name']=rename[a['name']]
            roots=[a['expr']] if a['kind'] in ('expr','let') else a['args']
            for root in roots:
                for e in walk(root):
                    if e['kind']=='var':e['name']=rename[e['name']]
                    elif e['kind']=='literal' and e['sort']=='String':
                        e['value']=f'ci-{i}/'+json.loads(e['text']);e['text']=json.dumps(e['value'])
            result.append(a)
    return result


def machine():
    if platform.system()=='Linux':
        cpu=next((line.split(':',1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')),platform.processor())
    else:cpu=subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip()
    return dict(os=platform.system(),arch=platform.machine(),cpu=cpu,python=platform.python_version(),
        rust=subprocess.check_output(['rustc','--version'],text=True).strip(),solver=solver.information(),
        runner_image=os.environ.get('ImageOS','local'),runner_image_version=os.environ.get('ImageVersion','unknown'))


def benchmark_case(name,source,out,copies,storage_cost='structural'):
    folder=out/name;folder.mkdir();bake=folder/'bake'
    print(f'{name}: Bake',flush=True)
    compiled_process=measure([sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--output',bake,'--skip-build','--storage-cost',storage_cost],folder/'compile')
    report=json.loads((bake/'report.json').read_text());data=json.loads((bake/'work/input.json').read_text())
    if report['status']!='compiled' or report['capture_mode']!='fresh':raise ValueError('CI requires a fresh successful Bake')
    library=(bake/'library.egg').read_text();runtime=(bake/'runtime.egg').read_text()
    init=folder/'initial.egg';init.write_text(runtime.removeprefix(library))
    parsed=c.ast(init,folder/'initial.json')
    generated=sum((json.loads((bake/'work'/f).read_text())['rules'] for f in ('marker-runtime.json','marker-exports.json')),[])
    check_renaming(data,generated)
    original=replicas([a['action'] for a in data['actions']],copies)
    projected=replicas([a['action'] for a in parsed['actions']],copies)
    env=folder/'native-env.egg';env.write_text('\n'.join(c.commands(data,{'declaration','rule'}))+'\n')
    native=folder/'native-input.egg';native.write_text('\n'.join(actions(original))+'\n')
    raw=folder/'raw-input.egg';raw.write_text('\n'.join(actions(original,True,data['signatures']))+'\n')
    projected_file=folder/'projected-input.egg';projected_file.write_text('\n'.join(actions(projected))+'\n')
    common=dict(constructors=sorted(data['signatures']))
    configs={
        'native':dict(common,environment=str(env),input=str(native),run='\n'.join(c.commands(data,{'schedule'}))),
        'raw_ingress':dict(common,environment=str(bake/'library.egg'),input=str(raw),run=markers.RUNTIME_SCHEDULE),
        'input_spine':dict(common,environment=str(bake/'library.egg'),input=str(projected_file),run=markers.RUNTIME_SCHEDULE),
    }
    configs['input_spine_export']=dict(configs['input_spine'],export='(run-schedule (saturate Export!))')
    item=dict(source=str(source.relative_to(ROOT)),source_sha256=c.digest(source),copies=copies,
        bake=dict(process_seconds=compiled_process['process_seconds'],phase_seconds=report['phase_seconds'],
            compiler_seconds=report['compiler_seconds'],capture_mode=report['capture_mode']),
        bytes={f:(bake/f).stat().st_size for f in ('library.egg','runtime.egg','restore.egg')},
        input_plan=report['input_plan']['status'],modes={})
    item['solver']=report['solver'];item['pricing_work']=report.get('pricing_work',{})
    item['cell_composition']=report.get('cell_composition',{'mode':'observed'})
    item['storage_pricing']=dict(mode=report.get('storage_cost','structural'),
        native_evaluations=(report.get('instance_storage') or {}).get('evaluations',0),
        seconds=(report.get('instance_storage') or {}).get('seconds',0.),
        seed_value_slots=report['plan']['costs'].get('instance_value_slots'))
    runner=ROOT/'target/release/examples/native_math_baseline'
    for mode,config in configs.items():
        timed_path=folder/f'{mode}.json';timed_path.write_text(json.dumps(dict(config,counts=False)))
        timing=measure([runner,'bench',timed_path],folder/f'{mode}-time');result=read_result(folder/f'{mode}-time')
        if result['verification'] is not None:raise ValueError('diagnostics leaked into the timing process')
        measured=dict(timing,**{k:result[k] for k in PHASES},matches=result['matches'])
        for stage in ('run','export'):
            for part,seconds in result[f'{stage}_engine_seconds'].items():measured[f'{stage}_{part}_seconds']=seconds
        inspect=dict(config,counts=True)
        if mode!='native':inspect['export']='(run-schedule (saturate Export!))'
        count_path=folder/f'{mode}-counts.json';count_path.write_text(json.dumps(inspect))
        measure([runner,'bench',count_path],folder/f'{mode}-counts')
        counted=read_result(folder/f'{mode}-counts');v=counted['verification']
        if counted['matches']!=result['matches']:raise ValueError('timed/diagnostic matches differ')
        if mode!='native' and v['original_before_export']['enodes']!=0:raise ValueError('original graph materialized during compact inference')
        item['modes'][mode]=dict(timing=measured,size=dict(
            physical_rows=v['native_total_rows_before_export'],raw_enodes=v['raw_spine']['enodes'],
            canonical_enodes=v['canonical_spine']['enodes'],active_markers=v['active_summaries'],
            retired_markers=v['subsumed_summaries'],restored_enodes=v['restored']['enodes'],
            restored_eclasses=v['restored']['eclasses'],tables={k:n for k,n in v['native_tables_before_export'].items() if n}))
    expected=item['modes']['native']['size']
    for mode,v in item['modes'].items():
        if any(v['size'][k]!=expected[k] for k in ('restored_enodes','restored_eclasses')):
            raise ValueError(f'{name}/{mode}: expanded size differs from same-input native execution')
    return item


def render(report):
    lines=['# Cell size and phase benchmark','',
        'Single run per mode; current egglog kernel, **not pristine**. Each seed is Baked once. Runtime replicas use a checked String-renaming symmetry; counts are checked against native execution, not inferred from template sizes.',
        '', '| Case | Mode | Pre-export rows | Raw / canonical nodes | Active / retired markers | Expanded nodes / classes | Peak RSS MiB |',
        '|---|---|---:|---:|---:|---:|---:|']
    for name,cse in report['cases'].items():
        for mode,row in cse['modes'].items():
            s,t=row['size'],row['timing']
            lines.append(f'| {name} × {cse["copies"]} | {mode} | {s["physical_rows"]} | {s["raw_enodes"]} / {s["canonical_enodes"]} | {s["active_markers"]} / {s["retired_markers"]} | {s["restored_enodes"]} / {s["restored_eclasses"]} | {t["peak_rss_bytes"]/2**20:.2f} |')
    lines+=['','## Runtime phases (ms)','',
        '| Case | Mode | Read | Install | Input | Run | Export | Drop | Execution total | Process wall |',
        '|---|---|---:|---:|---:|---:|---:|---:|---:|---:|']
    for name,cse in report['cases'].items():
        for mode,row in cse['modes'].items():
            t=row['timing'];values=[f'{t[k]*1000:.3f}' for k in PHASES+('process_seconds',)]
            lines.append('| '+' | '.join([name,mode,*values])+' |')
    lines+=['','## Engine-reported subphases (ms)','',
        '| Case | Mode | Run search + RHS | Run merge | Run rebuild | Export search + RHS | Export merge | Export rebuild |',
        '|---|---|---:|---:|---:|---:|---:|---:|']
    for name,cse in report['cases'].items():
        for mode,row in cse['modes'].items():
            lines.append('| '+' | '.join([name,mode,*[f'{row["timing"][k]*1000:.3f}' for k in ENGINE_PHASES]])+' |')
    lines+=['','These reuse egglog reports, without per-row timers. Search and RHS are combined. They are subsets of command phases; do not add them again to the execution total. Scheduler/preparation overhead is not separately attributed.']
    lines+=['','Execution total = install + input + run + export. Read, drop and process startup are separate. Deferred rule preparation can be charged to Run. RSS comes from a separate timed process without counts or graph export diagnostics.',
        '', '## Bake phases (seconds, one seed)','', '| Case | Phase | Seconds |','|---|---|---:|']
    for name,cse in report['cases'].items():
        for phase,value in cse['bake']['phase_seconds'].items():lines.append(f'| {name} | {phase} | {value:.6f} |')
        lines.append(f'| {name} | compiler / process total | {cse["bake"]["compiler_seconds"]:.6f} / {cse["bake"]["process_seconds"]:.6f} |')
    lines+=['','Spine solve includes conditional preparation, cost profiling and MIP solving; it is not solver CPU time alone. Capture includes native trace/ripen work. Build is performed before the benchmark and excluded.',
        '', '## Current-input storage pricing (one seed)', '',
        '| Case | Cost mode | Native pricing executions | Pricing seconds | Input/run checkpoint peak Value slots |',
        '|---|---|---:|---:|---:|']
    for name,cse in report['cases'].items():
        s=cse.get('storage_pricing',{})
        lines.append(f'| {name} | {s.get("mode","unavailable")} | {s.get("native_evaluations","—")} | {s.get("seconds","—")} | {s.get("seed_value_slots","—")} |')
    lines+=['','Pricing time is inside spine solve, not an additional runtime phase. Value slots include retained retired-marker rows but exclude index/allocation capacity. This is the seed cost, not the replicated workload or RSS.',
        '', '## Optimizer work (seconds)', '',
        '| Case | Model | Backend | Models / solves | API build | Solve | Peak variables / constraints |',
        '|---|---|---|---:|---:|---:|---:|']
    for name,cse in report['cases'].items():
        s=cse.get('solver',{})
        for model,m in s.get('models',{}).items():
            lines.append(f'| {name} | {model} | {s["backend"]} | {m["models"]} / {m["solve_calls"]} | {m["api_build_seconds"]:.6f} | {m["solve_seconds"]:.6f} | {m["peak_variables"]} / {m["peak_constraints"]} |')
    for name,cse in report['cases'].items():
        p=cse.get('pricing_work',{})
        lines.append(f'\n{name}: {p.get("layout_evaluations","unknown")} layout evaluations; {p.get("equivalent_model_hits","unknown")} exact conditional-model reuse hits; preparation {p.get("preparation_seconds","unknown")} s, layout profiling {p.get("layout_profile_seconds","unknown")} s.\n')
        lines.append(f'Pricing mode {p.get("mode","unknown")}; original product {p.get("original_cartesian_tuples","unknown")}; source compilations {p.get("source_preparations","unknown")}, reused {p.get("source_reuses","unknown")}; LHS normalizations {p.get("normalization_builds","unknown")}, reused {p.get("normalization_reuses","unknown")}.\n')
    lines+=['','Solver timers are subsets of Bake stages. API build excludes Python expression construction and lowering; solve includes native presolve/search. The full stage timers remain authoritative.',
        '', '## Generated files (bytes)','', '| Case | Library | Runtime | Restore |','|---|---:|---:|---:|']
    for name,cse in report['cases'].items():lines.append('| '+' | '.join([name,*[str(cse['bytes'][f]) for f in ('library.egg','runtime.egg','restore.egg')]])+' |')
    lines+=['','Full per-table counts, environment, binary hash and measurements are in `report.json`. Table rows, serialized bytes and RSS are different metrics; none is substituted for another.','']
    return '\n'.join(lines)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--suite',type=Path,default=ROOT/'tools/bench/ci_cases.json')
    p.add_argument('--cases',nargs='+');p.add_argument('--copies',type=int)
    p.add_argument('--storage-cost',choices=('structural','instances'),default='structural')
    a=p.parse_args();suite=json.loads(a.suite.read_text());copies=suite['copies'] if a.copies is None else a.copies
    if copies<1:p.error('copies must be positive')
    selected=a.cases or list(suite['cases'])
    if len(set(selected))!=len(selected) or set(selected)-set(suite['cases']):p.error('unknown or repeated case')
    os.environ.update(RAYON_NUM_THREADS='1',EGG_LAYOUT_GRAPHS='0',EGG_LAYOUT_EAGER_SVG='0',EGG_LAYOUT_ANALYSIS_THREADS='1',EGG_LAYOUT_QUERY_THREADS='1')
    limits=suite['ripen_limits']
    os.environ.update(EGG_LAYOUT_RIPEN_OBSERVE_MS=str(limits['milliseconds']),
        EGG_LAYOUT_RIPEN_OBSERVE_FACTS=str(limits['facts']),EGG_LAYOUT_RIPEN_SWEEP_STEPS=str(limits['sweep_steps']))
    for k in ('EGG_LAYOUT_FULL_TRACE','EGG_LAYOUT_RIPEN_HISTORY','EGG_LAYOUT_PROFILE','EGGLOG_JOIN_WORK_RULES'):os.environ.pop(k,None)
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    report=dict(schema='cell-ci-bench/v2',suite_sha256=c.digest(a.suite),copies=copies,threads=1,ripen_limits=limits,environment=machine(),storage_cost=a.storage_cost,
        commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True)),
        binary_sha256=c.digest(ROOT/'target/release/examples/native_math_baseline'),cases={})
    for name in selected:
        report['cases'][name]=benchmark_case(name,ROOT/suite['cases'][name],out,copies,a.storage_cost)
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        (out/'summary.md').write_text(render(report))
        print(f'{name}: complete',flush=True)
    print(out/'summary.md')


if __name__=='__main__':main()
