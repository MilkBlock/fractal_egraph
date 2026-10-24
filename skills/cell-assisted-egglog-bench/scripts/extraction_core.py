"""Complete in-memory Tiger timing; reuse pristine inference and native oracle receipts."""
import csv
import json
import math
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

from extraction_suite import cached_process, clean_env, sha, source_provenance, write

PROTOCOL='tiger-extract-all-fun-roots/v1'


def replace_once(text, old, new):
    if text.count(old)!=1:raise ValueError(f'Tiger timing hook changed: {old}')
    return text.replace(old,new)


def instrument_main(text):
    """Clock the actual function, not process time minus estimated I/O."""
    text='#include <chrono>\n'+text
    parse='pair<EGraph, vector<EClassId> > res = parse_egglog_json();'
    text=replace_once(text,parse,
        'using TimingClock = std::chrono::steady_clock;\n'
        '    auto parse_start = TimingClock::now();\n    '+parse+
        '\n    auto parse_end = TimingClock::now();\n'
        '    double extraction_seconds = -1;')
    call='extractions = extract_all_fun_roots_tiger(g, roots);'
    text=replace_once(text,call,
        'auto extraction_start = TimingClock::now();\n        '+call+
        '\n        auto extraction_end = TimingClock::now();\n'
        '        extraction_seconds = std::chrono::duration<double>(extraction_end-extraction_start).count();')
    output='output_egglog(g, extractions);'
    text=replace_once(text,output,
        'auto output_start = TimingClock::now();\n    '+output+
        '\n    std::fflush(stdout);\n    auto output_end = TimingClock::now();\n'
        '    std::fprintf(stderr, "TIGER_EXTRACTION_TIMING {\\"read_parse_seconds\\":%.9f,'
        '\\"algorithm_seconds\\":%.9f,\\"serialize_write_seconds\\":%.9f}\\n",\n'
        '        std::chrono::duration<double>(parse_end-parse_start).count(), extraction_seconds,\n'
        '        std::chrono::duration<double>(output_end-output_start).count());')
    return text


def remove_phase_prints(text):
    # The old diagnostic writes would be inside the measured algorithm call.
    lines=text.splitlines(keepends=True)
    removed=[line for line in lines if 'std::fprintf(stderr, "TIGER_COST_PHASE ' in line]
    if len(removed)!=2:raise ValueError('Cost-phase instrumentation changed')
    return ''.join(line for line in lines if line not in removed)


def build_timed(checkout,build_dir,repo):
    sys.path.insert(0,str(repo))
    from tools.tiger_markers.build import build, SOURCES
    overlay,_=build(checkout,build_dir/'bag',metal=False)
    base=json.loads((overlay.parent/'build.json').read_text())
    compiler=subprocess.check_output(['c++','--version'],text=True).splitlines()[0]
    flags=['-std=c++17','-O3','-Wno-unused-result']
    result={}
    for mode,source in [('native',checkout/'dag_in_context/src/tiger'),('bag',overlay.parent/'source')]:
        identity=dict(protocol=PROTOCOL,compiler=compiler,flags=flags,
                      instrumenter=sha(Path(__file__)),source={p.name:sha(p) for p in sorted(source.iterdir()) if p.suffix in ('.h','.cpp')})
        folder=build_dir/('core-'+mode);stamp=folder/'build.json';binary=folder/'tiger'
        if stamp.exists():
            old=json.loads(stamp.read_text())
            if old['identity']!=identity or old['binary_sha256']!=sha(binary):raise ValueError('Core build fingerprint changed; use a fresh build directory')
        else:
            target=folder/'source';target.mkdir(parents=True,exist_ok=False)
            for p in source.iterdir():
                if p.suffix in ('.h','.cpp'):shutil.copy2(p,target/p.name)
            main=target/'main.cpp';main.write_text(instrument_main(main.read_text()))
            if mode=='bag':
                greedy=target/'greedy.cpp';greedy.write_text(remove_phase_prints(greedy.read_text()))
            subprocess.run(['c++',*flags,*SOURCES,'-o',str(binary)],cwd=target,check=True)
            write(stamp,dict(identity=identity,binary_sha256=sha(binary)))
        result[mode]=binary
    return result,base


def timing(row):
    records=[s for s in row['stats'] if 'algorithm_seconds' in s]
    if len(records)!=1:raise ValueError('Expected exactly one whole-algorithm timer')
    t=records[0]
    if row['returncode']!=0 or row['timeout'] or t['algorithm_seconds']<=0:
        raise ValueError('Incomplete Tiger extraction')
    parts=[t[k] for k in ['read_parse_seconds','algorithm_seconds','serialize_write_seconds']]
    if any(v<0 for v in parts) or sum(parts)>row['wall_seconds']+1e-6:
        raise ValueError('Internal timing intervals do not fit process elapsed time')
    return t


def summarize(out):
    manifest=json.loads((out/'manifest.json').read_text())
    cases=[json.loads(p.read_text()) for p in sorted((out/'cases').glob('*/result.json'))]
    rows=[]
    for c in cases:
        passed=len(c['graphs'])==3 and all(g['passed'] for g in c['graphs'])
        row=dict(name=c['name'],suite=c['suite'],source=c['source'],passed=passed)
        for mode in ['native','bag']:
            for key in ['read_parse_seconds','algorithm_seconds','serialize_write_seconds']:
                row[f'{mode}_{key}']=sum(g['timing'][mode][key] for g in c['graphs']) if passed else None
            row[f'{mode}_wall_seconds']=sum(g['runs'][mode]['wall_seconds'] for g in c['graphs']) if passed else None
        row['algorithm_speedup']=row['native_algorithm_seconds']/row['bag_algorithm_seconds'] if passed else None
        rows.append(row)
    valid=[r for r in rows if r['passed']]
    gm=lambda xs:math.exp(sum(map(math.log,xs))/len(xs)) if xs else None
    totals={key:sum(r[key] for r in valid) for key in rows[0] if key.endswith('_seconds')} if rows else {}
    summary=dict(schema=PROTOCOL,manifest=manifest,inventory=manifest['programs'],attempted=len(rows),
                 passed=len(valid),passed_graphs=sum(g['passed'] for c in cases for g in c['graphs']),rows=rows,totals=totals,
                 algorithm_geomean=gm([r['algorithm_speedup'] for r in valid]),
                 algorithm_aggregate=totals['native_algorithm_seconds']/totals['bag_algorithm_seconds'] if valid else None,
                 wins=sum(r['algorithm_speedup']>1 for r in valid),regressions=[r['name'] for r in valid if r['algorithm_speedup']<1],
                 scope='One paired run per saved graph. In-memory extract_all_fun_roots_tiger includes all region, cost, Statewalk and DAG work; excludes input read/parse, output serialization/write, process startup/teardown. New outputs must equal both saved pristine and saved oracle output. Historical cost oracle reused only for identical overlay inputs.')
    write(out/'summary.json',summary)
    if rows:
        with (out/'workloads.csv').open('w',newline='') as f:
            writer=csv.DictWriter(f,fieldnames=list(rows[0]),lineterminator='\n');writer.writeheader();writer.writerows(rows)
    return summary


def run(a,repo):
    resolve=lambda p:p.resolve() if p.is_absolute() else (repo/p).resolve()
    suite,checkout,out,build_dir=map(resolve,[a.suite,a.checkout,a.output,a.build_dir])
    if a.dry_run:
        print('Time complete in-memory extraction on existing suite graphs; no inference rerun:',suite);return 0
    if a.report_only:
        print(json.dumps({k:v for k,v in summarize(out).items() if k not in ['rows','manifest']},indent=2));return 0
    prior=json.loads((suite/'manifest.json').read_text())
    previous=[json.loads(p.read_text()) for p in sorted((suite/'cases').glob('*/result.json'))]
    provenance=source_provenance(checkout)
    if provenance!=prior['provenance']:raise ValueError('Pristine source/dependency provenance differs')
    if len(previous)!=len(prior['inventory']):raise ValueError('Original suite is incomplete')
    binaries,base=build_timed(checkout,build_dir,repo)
    # Both the historical oracle and this candidate use this exact overlay.
    if base['tiger']!=prior['candidate']:raise ValueError('Cannot reuse cost oracle: candidate binary changed')
    affinity=sorted(os.sched_getaffinity(0)) if hasattr(os,'sched_getaffinity') else None
    machine=platform.uname()._asdict()
    if affinity!=prior['affinity'] or machine!=prior['machine']:raise ValueError('Different machine or affinity from original suite')
    inputs=[]
    for c in previous:
        if not c['all_frozen_checks_passed'] or not c['complete_native_pipeline'] or len(c['graphs'])!=3:
            raise ValueError(f'Original quality gate failed: {c["name"]}')
        for info,g in zip(c['pipeline']['native']['graphs'],c['graphs']):
            path=suite/'cases'/c['name']/'native/graphs'/info['file']
            if sha(path)!=g['input_sha256'] or info['sha256']!=g['input_sha256']:raise ValueError(f'Input graph changed: {path}')
            oracle=g['runs']['oracle']
            if oracle['returncode']!=0 or oracle['stdout_sha256']!=g['runs']['native']['stdout_sha256'] or not any(s.get('passed') is True for s in oracle['stats']):
                raise ValueError('Historical oracle receipt is invalid')
            inputs.append(dict(case=c['name'],pass_number=g['pass'],sha256=sha(path)))
    identity=dict(protocol=PROTOCOL,provenance=provenance,prior_manifest_sha256=sha(suite/'manifest.json'),
                  original_suite=str(suite),programs=len(previous),graphs=inputs,binaries={k:sha(v) for k,v in binaries.items()},
                  runner=sha(Path(__file__)),common_runner=sha(Path(__file__).with_name('extraction_suite.py')),
                  timeout=a.timeout,machine=machine,affinity=affinity)
    out.mkdir(parents=True,exist_ok=True);stamp=out/'manifest.json'
    if stamp.exists() and json.loads(stamp.read_text())!=identity:raise ValueError('Core measurement inputs changed; use a fresh output directory')
    write(stamp,identity)
    for ci,c in enumerate(previous):
        result=dict(name=c['name'],suite=c['suite'],source=c['source'],graphs=[])
        folder=out/'cases'/c['name'];folder.mkdir(parents=True,exist_ok=True)
        for pi,(info,g) in enumerate(zip(c['pipeline']['native']['graphs'],c['graphs'])):
            path=suite/'cases'/c['name']/'native/graphs'/info['file'];runs={};times={}
            for mode in (['native','bag'] if (ci+pi)%2==0 else ['bag','native']):
                runs[mode]=cached_process([binaries[mode],'--ilp-solver','cbc'],folder/f'pass-{pi+1}'/mode,
                    stdin=path,env=clean_env(None if mode=='native' else 'bag'),timeout=a.timeout,tiger_limit=True,retry=a.retry_failed)
            error=None
            try:times={mode:timing(row) for mode,row in runs.items()}
            except ValueError as e:error=str(e)
            passed=error is None and all(r['stdout_sha256']==g['runs']['native']['stdout_sha256'] for r in runs.values())
            result['graphs'].append(dict(pass_number=pi+1,input_sha256=g['input_sha256'],reference_output_sha256=g['runs']['native']['stdout_sha256'],
                reused_oracle=g['runs']['oracle']['stats'],passed=passed,error=error,runs=runs,timing=times))
            write(folder/'result.json',result)
        print(f'[{ci+1}/{len(previous)}] {c["source"]}: {sum(g["passed"] for g in result["graphs"])}/3 exact',flush=True)
    s=summarize(out)
    print(json.dumps({k:s[k] for k in ['inventory','passed','passed_graphs','algorithm_geomean','algorithm_aggregate','totals','regressions']},indent=2))
    return 0 if s['passed']==s['inventory'] else 1
