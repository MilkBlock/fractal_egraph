"""Whole-suite extraction study on an unmodified, locked upstream eggcc.

Runs the real three-pass pipeline and compares each captured graph in isolation.
Only the sibling Tiger executable changes; the eggcc binary is identical.
"""
import csv
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import resource
import shutil
import signal
import subprocess
import sys
import threading
import time
import tomllib

from extraction_markers import sha


def write(path, value):
    path=Path(path);temporary=path.with_suffix(path.suffix+'.tmp')
    temporary.write_text(json.dumps(value,indent=2,ensure_ascii=False)+'\n')
    temporary.replace(path)


def clean_env(mode=None, verify=False):
    env={k:v for k,v in os.environ.items() if not k.startswith(('TIGER_EXTRACTION_','CELL_','EGGCC_PASS_'))}
    env.update(RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1')
    if mode:env.update(TIGER_EXTRACTION_MARKERS=mode,TIGER_EXTRACTION_VERIFY='1' if verify else '0')
    return env


def seconds(value):
    if value is None:return None
    return value['secs']+value['nanos']/1e9 if isinstance(value,dict) else float(value)


def run_process(command, folder, *, stdin=None, env=None, timeout=600, tiger_limit=False):
    """Per-process RSS, not the cumulative RUSAGE_CHILDREN high-water mark."""
    folder.mkdir(parents=True,exist_ok=True)
    actual=list(map(str,command));timer=folder/'resource.txt'
    if platform.system()=='Linux':actual=['/usr/bin/time','-f','%e %U %S %M','-o',str(timer),*actual]
    def limits():
        resource.setrlimit(resource.RLIMIT_CORE,(0,0))
        if tiger_limit:resource.setrlimit(resource.RLIMIT_AS,(8*1024**3,8*1024**3))
    inp=open(stdin,'rb') if stdin else open(os.devnull,'rb')
    with inp,(folder/'stdout').open('wb') as out,(folder/'stderr').open('wb') as err:
        start=time.perf_counter()
        p=subprocess.Popen(actual,stdin=inp,stdout=out,stderr=err,env=env,
                           start_new_session=True,preexec_fn=limits)
        expired=threading.Event()
        def kill_late():
            if p.poll() is None:
                expired.set()
                try:os.killpg(p.pid,signal.SIGKILL)
                except ProcessLookupError:pass
        watchdog=threading.Timer(timeout,kill_late);watchdog.daemon=True;watchdog.start()
        try:
            # wait(timeout=...) polls with sleeps on POSIX and can add 50 ms to
            # a short extractor. Blocking wait preserves the measurement.
            p.wait()
            elapsed=time.perf_counter()-start
        finally:watchdog.cancel();watchdog.join()
        timed_out=expired.is_set()
    result=dict(command=list(map(str,command)),returncode=p.returncode,timeout=timed_out,
                wall_seconds=elapsed,stdout_sha256=sha(folder/'stdout'),max_rss_kib=None,
                measured_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
    if timer.exists():
        parts=timer.read_text().splitlines()[-1].split()
        if len(parts)==4:
            try:result.update(user_seconds=float(parts[1]),system_seconds=float(parts[2]),max_rss_kib=int(parts[3]))
            except ValueError:pass
    result['stats']=[]
    for line in (folder/'stderr').read_text(errors='replace').splitlines():
        if line.startswith(('TIGER_COST_PHASE ','TIGER_COST_BAG ','TIGER_GREEDY_ORACLE ')):
            result['stats'].append(json.loads(line.split(' ',1)[1]))
    write(folder/'measurement.json',result)
    return result


def cached_process(command, folder, *, stdin=None, env=None, timeout=600, tiger_limit=False, retry=False):
    identity=dict(command=list(map(str,command)),input=sha(stdin) if stdin else None,
                  environment={k:v for k,v in (env or {}).items() if k.startswith(('TIGER_','RAYON_','OMP_','OPENBLAS_'))},
                  timeout=timeout,tiger_limit=tiger_limit)
    stamp=folder/'identity.json';measurement=folder/'measurement.json'
    if stamp.exists() and json.loads(stamp.read_text())!=identity:raise ValueError(f'Changed measurement inputs: {folder}')
    if measurement.exists():
        old=json.loads(measurement.read_text())
        if (old['returncode']==0 or not retry) and (folder/'stdout').exists() and sha(folder/'stdout')==old['stdout_sha256']:
            return dict(old,reused=True)
        if old['returncode']!=0 and retry:
            archive=folder.parent/'attempts'/f'{folder.name}-{time.time_ns()}'
            archive.parent.mkdir(parents=True,exist_ok=True);folder.rename(archive)
    folder.mkdir(parents=True,exist_ok=True);write(stamp,identity)
    return run_process(command,folder,stdin=stdin,env=env,timeout=timeout,tiger_limit=tiger_limit)


def source_provenance(checkout):
    dirty=subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=checkout,text=True).strip()
    if dirty:raise ValueError(f'eggcc source is not pristine: {dirty}')
    rustc=subprocess.check_output(['rustc','-vV'],cwd=checkout,text=True)
    host=next(line.split(': ',1)[1] for line in rustc.splitlines() if line.startswith('host: '))
    metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--offline','--format-version','1','--filter-platform',host],cwd=checkout))
    deps={};seen=set()
    for p in metadata['packages']:
        if not (p.get('source') or '').startswith('git+'):continue
        root=subprocess.check_output(['git','rev-parse','--show-toplevel'],cwd=Path(p['manifest_path']).parent,text=True).strip()
        if root not in seen:
            seen.add(root)
            diff=subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=root,text=True).strip()
            if diff:raise ValueError(f'Modified dependency: {root}: {diff}')
        revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
        if revision!=p['source'].rsplit('#',1)[1]:raise ValueError(f'Dependency revision mismatch: {p["name"]}')
        deps[p['name']]=dict(source=p['source'],revision=revision)
    lock=tomllib.loads((checkout/'Cargo.lock').read_text())
    core={p['name']:dict(version=p['version'],source=p.get('source')) for p in lock['package']
          if p['name'] in ('egglog','egglog-experimental','bril-rs')}
    return dict(eggcc_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=checkout,text=True).strip(),
                cargo_lock_sha256=sha(checkout/'Cargo.lock'),locked_engine=core,git_dependencies=deps,rustc=rustc,
                tracked_sources_clean=True,
                definition='Pristine upstream eggcc and its exact Cargo.lock engine stack; no local Cell/GPU/tracing engine patches.')


def build_tools(checkout, build_dir, repo):
    sys.path.insert(0,str(repo))
    from tools.tiger_markers.build import build, SOURCES, HERE
    build_dir.mkdir(parents=True,exist_ok=True)
    source=checkout/'dag_in_context/src/tiger';native=build_dir/'native-tiger'
    inputs={p.name:sha(p) for p in sorted(source.iterdir()) if p.suffix in ('.h','.cpp')}
    compiler=subprocess.check_output(['c++','--version'],text=True).splitlines()[0]
    identity=dict(source=inputs,compiler=compiler,flags=['-std=c++17','-O3','-Wno-unused-result'],sources=SOURCES)
    stamp=build_dir/'native-build.json'
    if stamp.exists():
        old=json.loads(stamp.read_text())
        if old['identity']!=identity or old['sha256']!=sha(native):raise ValueError('Native build cache changed')
    else:
        subprocess.run(['c++',*identity['flags'],*SOURCES,'-o',str(native)],cwd=source,check=True)
        write(stamp,dict(identity=identity,sha256=sha(native)))
    candidate,worker=build(checkout,build_dir/'bag',metal=False)
    assert worker is None
    test=build_dir/'cost-tests';test_log=build_dir/'cost-tests.log'
    test_id=dict(test_source=sha(HERE/'test_costs.cpp'),candidate=sha(candidate))
    if not (build_dir/'cost-tests.json').exists():
        overlay=candidate.parent/'source'
        subprocess.run(['c++','-std=c++17','-O2','-Wno-unused-result','-I',str(overlay),str(HERE/'test_costs.cpp'),
                        str(overlay/'greedy.cpp'),str(overlay/'egraphin.cpp'),'-o',str(test)],check=True)
        with test_log.open('w') as f:subprocess.run([str(test)],stdout=f,stderr=subprocess.STDOUT,check=True)
        write(build_dir/'cost-tests.json',test_id)
    elif json.loads((build_dir/'cost-tests.json').read_text())!=test_id:raise ValueError('Cost-test inputs changed')
    return native,candidate


def lane_binaries(binary,native,candidate,out):
    result={}
    for mode,tiger in [('native',native),('bag',candidate)]:
        folder=out/'bin'/mode;folder.mkdir(parents=True,exist_ok=True)
        for src,dst in [(binary,folder/'eggcc'),(tiger,folder/'tiger')]:
            if dst.exists():
                if sha(dst)!=sha(src):raise ValueError(f'Changed lane executable: {dst}')
            else:shutil.copy2(src,dst)
        result[mode]=folder/'eggcc'
    assert sha(result['native'])==sha(result['bag'])
    return result


def phase(row,key):
    values=[s[key] for s in row.get('stats',[]) if key in s]
    return sum(values) if values else None


def graph_pair(path,folder,native,candidate,a,index):
    rows={};order=['native','bag'] if index%2==0 else ['bag','native']
    for mode in order:
        binary=native if mode=='native' else candidate
        rows[mode]=cached_process([binary,'--ilp-solver','cbc'],folder/mode,stdin=path,
                                 env=clean_env(None if mode=='native' else 'bag'),timeout=a.timeout,
                                 tiger_limit=True,retry=a.retry_failed)
    # Separate diagnostic/oracle lanes are never included in the speedup.
    for mode,verify,name in [('off',False,'phase-reference'),('bag',True,'oracle')]:
        rows[name]=cached_process([candidate,'--ilp-solver','cbc'],folder/name,stdin=path,
                                 env=clean_env(mode,verify),timeout=a.timeout,tiger_limit=True,retry=a.retry_failed)
    passed=all(r['returncode']==0 for r in rows.values()) and len({r['stdout_sha256'] for r in rows.values()})==1
    oracle=[s for s in rows['oracle']['stats'] if s.get('passed') is True and 'classes' in s]
    passed=passed and bool(oracle)
    with path.open() as f:g=json.load(f)
    graph_stats=dict(serialized_nodes=len(g['nodes']),serialized_eclasses=len({n['eclass'] for n in g['nodes'].values()}),
                     functions=sum(n['op']=='Function' for n in g['nodes'].values()),
                     max_arity=max((len(n['children']) for n in g['nodes'].values()),default=0))
    result=dict(input_sha256=sha(path),graph=graph_stats,runs=rows,quality_passed=passed,
                native_estimate_seconds=phase(rows['phase-reference'],'estimate_seconds'),
                bag_estimate_seconds=phase(rows['bag'],'estimate_seconds'),
                selected_output_bindings=sum('(let __tmp' in l for l in (folder/'native/stdout').read_text(errors='replace').splitlines()),
                speedup=rows['native']['wall_seconds']/rows['bag']['wall_seconds'] if passed else None)
    write(folder/'result.json',result)
    return result


def case(source,checkout,out,lanes,native,candidate,a,index):
    relative=source.relative_to(checkout/'benchmarks/passing');name='__'.join(relative.with_suffix('').parts)
    folder=out/'cases'/name;folder.mkdir(parents=True,exist_ok=True)
    result=dict(name=name,suite=relative.parts[0],source=str(relative),source_sha256=sha(source),pipeline={},graphs=[])
    order=['native','bag'] if index%2==0 else ['bag','native']
    for mode in order:
        dest=folder/mode;dest.mkdir(exist_ok=True);graphs=dest/'graphs';graphs.mkdir(exist_ok=True)
        data=dest/'run-data.json'
        command=[lanes[mode],source,'--run-mode','optimized-pretty-print','--eggcc-schedule','parallel',
                 '--run-data-out',data,'--egraph-out-dir',graphs]
        row=cached_process(command,dest,env=clean_env(None if mode=='native' else 'bag'),timeout=a.timeout,retry=a.retry_failed)
        if row['returncode']==0 and data.exists():
            report=json.loads(data.read_text())
            for key in ['eggcc_compile_time','eggcc_extraction_time','eggcc_serialization_time']:
                row[key+'_seconds']=seconds(report[key])
        row['graphs']=[dict(file=p.name,sha256=sha(p)) for p in sorted(graphs.glob('tiger_egraph_*.json'))]
        result['pipeline'][mode]=row;write(folder/'result.json',result)
    na,ba=[result['pipeline'][m] for m in ['native','bag']]
    result['pipeline_output_equal']=na['returncode']==ba['returncode']==0 and na['stdout_sha256']==ba['stdout_sha256']
    result['intermediate_graphs_byte_equal']=bool(na['graphs']) and na['graphs']==ba['graphs']
    result['pipeline_speedup_eligible']=result['pipeline_output_equal'] and result['intermediate_graphs_byte_equal']
    result['pipeline_speedup']=na['wall_seconds']/ba['wall_seconds'] if result['pipeline_speedup_eligible'] else None
    for i,g in enumerate(na['graphs']):
        path=folder/'native/graphs'/g['file']
        pair=graph_pair(path,folder/'frozen'/f'pass-{i+1}',native,candidate,a,index+i)
        pair['pass']=i+1;result['graphs'].append(pair);write(folder/'result.json',result)
    result['complete_native_pipeline']=na['returncode']==0 and len(na['graphs'])==3
    result['all_frozen_checks_passed']=bool(result['graphs']) and all(g['quality_passed'] for g in result['graphs'])
    result['completed']=True;write(folder/'result.json',result)
    return result


def summarize(out):
    manifest=json.loads((out/'manifest.json').read_text())
    cases=[json.loads(p.read_text()) for p in sorted((out/'cases').glob('*/result.json'))]
    def gm(xs):return math.exp(sum(math.log(x) for x in xs)/len(xs)) if xs else None
    rows=[]
    for c in cases:
        graphs=c['graphs'];passed=c.get('complete_native_pipeline') and c.get('all_frozen_checks_passed') and len(graphs)==3
        native=sum(g['runs']['native']['wall_seconds'] for g in graphs) if passed else None
        bag=sum(g['runs']['bag']['wall_seconds'] for g in graphs) if passed else None
        old=sum(g['native_estimate_seconds'] for g in graphs) if passed and all(g['native_estimate_seconds'] is not None and g['bag_estimate_seconds'] is not None for g in graphs) else None
        new=sum(g['bag_estimate_seconds'] for g in graphs) if old is not None else None
        rss={mode:max((g['runs'][mode]['max_rss_kib'] for g in graphs if g['runs'][mode]['max_rss_kib'] is not None),default=0)/1024 or None for mode in ['native','bag']}
        rows.append(dict(name=c['name'],suite=c['suite'],source=c['source'],completed=c.get('completed',False),
                         native_complete=c.get('complete_native_pipeline',False),frozen_checked=passed,
                         native_extraction_seconds=native,bag_extraction_seconds=bag,extraction_speedup=native/bag if passed else None,
                         native_estimate_seconds=old,bag_estimate_seconds=new,estimate_speedup=old/new if new else None,
                         native_peak_rss_mib=rss['native'],bag_peak_rss_mib=rss['bag'],
                         rss_ratio=rss['native']/rss['bag'] if passed and all(rss.values()) else None,
                         max_serialized_nodes=max((g['graph']['serialized_nodes'] for g in graphs),default=0),
                         max_serialized_eclasses=max((g['graph']['serialized_eclasses'] for g in graphs),default=0),
                         bag_merges=sum(phase(g['runs']['bag'],'merges') or 0 for g in graphs),
                         native_pipeline_seconds=c['pipeline'].get('native',{}).get('wall_seconds'),
                         bag_pipeline_seconds=c['pipeline'].get('bag',{}).get('wall_seconds'),
                         pipeline_speedup=c.get('pipeline_speedup'),pipeline_output_equal=c.get('pipeline_output_equal'),
                         intermediate_graphs_byte_equal=c.get('intermediate_graphs_byte_equal'),
                         native_returncode=c['pipeline'].get('native',{}).get('returncode'),
                         bag_returncode=c['pipeline'].get('bag',{}).get('returncode'),
                         native_timeout=c['pipeline'].get('native',{}).get('timeout'),bag_timeout=c['pipeline'].get('bag',{}).get('timeout')))
    valid=[r for r in rows if r['frozen_checked']]
    summary=dict(schema='pristine-eggcc-extraction-suite/v1',manifest=manifest,rows=rows,
                 inventory=len(manifest['inventory']),attempted=len(rows),completed=sum(r['completed'] for r in rows),
                 frozen_checked_programs=len(valid),frozen_checked_graphs=sum(g['quality_passed'] for c in cases for g in c['graphs']),
                 extraction_geomean=gm([r['extraction_speedup'] for r in valid]),
                 extraction_aggregate_speedup=sum(r['native_extraction_seconds'] for r in valid)/sum(r['bag_extraction_seconds'] for r in valid) if valid else None,
                 estimate_geomean=gm([r['estimate_speedup'] for r in valid if r['estimate_speedup'] is not None]),
                 pipeline_geomean=gm([r['pipeline_speedup'] for r in rows if r['pipeline_speedup'] is not None]),
                 regressions=[r['name'] for r in valid if r['extraction_speedup']<1],
                 scope='Single paired measurements; builds and verification excluded. Frozen-graph extraction ratios require exact complete output and native cost/pick oracle. Pipeline timings include upstream graph capture; ratios additionally require identical intermediate graph bytes. No Cell/GPU inference, ILP run, or executable-cycle claim.')
    write(out/'summary.json',summary)
    if rows:
        with (out/'workloads.csv').open('w',newline='') as f:
            writer=csv.DictWriter(f,fieldnames=list(rows[0]));writer.writeheader();writer.writerows(rows)
    return summary


def run(a,repo):
    resolve=lambda p:p.resolve() if p.is_absolute() else (repo/p).resolve()
    checkout,out,build_dir=map(resolve,[a.checkout,a.output,a.build_dir])
    if a.dry_run:
        print('Unmodified locked eggcc; all 96 upstream passing files; sequential native/bag pipelines and exact frozen-graph checks:',out)
        return 0
    if a.report_only:
        s=summarize(out);print(json.dumps({k:s[k] for k in ['attempted','completed','frozen_checked_programs','extraction_geomean']},indent=2));return 0
    provenance=source_provenance(checkout)
    binary=checkout/'target/release/eggcc'
    native,candidate=build_tools(checkout,build_dir,repo)
    all_sources=sorted(p for p in (checkout/'benchmarks/passing').rglob('*') if p.suffix in ('.rs','.bril'))
    inventory={str(p.relative_to(checkout/'benchmarks/passing')):sha(p) for p in all_sources}
    identity=dict(provenance=provenance,binary=sha(binary),native=sha(native),candidate=sha(candidate),
                  timing_protocol='blocking-wait-with-separate-watchdog/v2',
                  runner=sha(Path(__file__)),inventory=inventory,timeout=a.timeout,
                  machine=platform.uname()._asdict(),affinity=sorted(os.sched_getaffinity(0)) if hasattr(os,'sched_getaffinity') else None,
                  compiler=subprocess.check_output(['c++','--version'],text=True).splitlines()[0])
    out.mkdir(parents=True,exist_ok=True)
    manifest=out/'manifest.json'
    if manifest.exists() and json.loads(manifest.read_text())!=identity:raise ValueError('Suite provenance changed; choose a fresh output directory')
    write(manifest,identity)
    lanes=lane_binaries(binary,native,candidate,out)
    selected=[(p if p.is_absolute() else checkout/p).resolve() for p in a.source] if a.source else all_sources
    for i,source in enumerate(selected):
        if source not in all_sources:raise ValueError(f'Not in upstream passing inventory: {source}')
        print(f'[{i+1}/{len(selected)}] {source.relative_to(checkout)}',flush=True)
        case(source,checkout,out,lanes,native,candidate,a,all_sources.index(source))
        s=summarize(out);print(f'  completed={s["completed"]}; exact frozen checks={s["frozen_checked_programs"]}',flush=True)
    s=summarize(out)
    print(json.dumps({k:s[k] for k in ['inventory','attempted','completed','frozen_checked_programs','extraction_geomean','pipeline_geomean']},indent=2))
    return 0 if s['attempted']==s['frozen_checked_programs'] else 1
