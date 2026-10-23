"""All passing eggcc inputs, actual pass dumps, streamed per-Run work profiles.

This is diagnostic work, not a timed benchmark. No history or per-binding values
are saved. Failed workloads stay in the manifest and never acquire zero cost.
"""
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def sha(p):
    h=hashlib.sha256()
    with Path(p).open('rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
    return h.hexdigest()


def save(path,value):
    tmp=path.with_suffix('.tmp');tmp.write_text(json.dumps(value,indent=2)+'\n');tmp.replace(path)


def run(a,repo):
    resolve=lambda p:p if p.is_absolute() else repo/p
    production=resolve(a.production_repo);out=resolve(a.output)
    capture=resolve(a.profile_binary);flatten=production/'target/release/flatten'
    probe=repo/'target/analyzer/release/examples/planner_profile'
    sources=sorted(resolve(p) for p in a.source) if a.source else sorted(p for p in (production/'benchmarks/passing').rglob('*') if p.suffix in ('.bril','.rs'))
    ids=[dict(source=str(p),sha256=sha(p)) for p in sources]
    if not sources:raise ValueError('empty workload corpus')
    if a.dry_run:
        print('Actual production pass chain for',len(sources),'inputs; native pass replays with every admitted fixed query anchor, every Run; compressed count-only profiles.');return 0
    if not a.no_build:
        subprocess.run(['cargo','build','--offline','--release','--features','planner-profile','--example','planner_profile','--target-dir','target/analyzer'],cwd=repo,check=True)
    for p in [capture,flatten,probe]:
        if not p.is_file():raise FileNotFoundError(p)
    origin=json.loads((capture.parent/'provenance.json').read_text())
    if origin['profile_binary_sha256']!=sha(capture):raise ValueError('production profiler fingerprint differs')
    current_driver=sha(production/'dag_in_context/src/lib.rs')
    if current_driver!=origin['original_driver_sha256']:raise ValueError('production driver differs from cached profiler; build a fresh profiler')
    identity=dict(workloads=ids,production_profiler=sha(capture),flatten=sha(flatten),probe=sha(probe),production_provenance=origin)
    manifest=out/'manifest.json'
    if out.exists():
        if not a.resume:raise ValueError('existing corpus needs --resume')
        state=json.loads(manifest.read_text())
        if state['identity']!=identity:raise ValueError('corpus tools/inputs changed; use a new output directory')
    else:
        out.mkdir(parents=True);state=dict(schema='marker-workload-corpus/v1',identity=identity,cases=[],complete=False,
            scope='actual production extraction feeds each subsequent pass; count-only replay in target native engine, not production or GPU timing')
        save(manifest,state)
    by_source={c['source']:c for c in state['cases']}
    for number,source in enumerate(sources):
        key=str(source)
        case=by_source.get(key)
        if case and (case['status']=='complete' or (case['status']=='failed' and not a.retry_failed)):continue
        folder=out/f'{number:03}-{source.stem}'
        folder.mkdir(exist_ok=True)
        if case is None:
            case=dict(source=key,source_sha256=sha(source),directory=str(folder),status='running',passes=[])
            state['cases'].append(case);by_source[key]=case
        elif case['status']=='failed':
            case.setdefault('failures',[]).append(case.pop('error','unknown failure'))
            case['status']='running';state['finished']=False
        print(f'[{number+1}/{len(sources)}] {source.name}',flush=True);save(manifest,state)
        try:
            captured=folder/'capture';capture_done=folder/'capture-complete.json'
            if not capture_done.exists():
                if captured.exists():raise ValueError('incomplete production capture retained; inspect before retry')
                command=[sys.executable,str(repo/'experiments/eggcc_passes/run.py'),'--timing-only','--production-repo',str(production),
                    '--profile-binary',str(capture),'--source',key,'--output',str(captured)]
                with (folder/'capture.log').open('w') as log:
                    result=subprocess.run(command,cwd=repo,stdout=log,stderr=subprocess.STDOUT)
                if result.returncode:raise ValueError('production capture failed; see capture.log')
                save(capture_done,dict(command=command))
            dumps=sorted((captured/source.stem).glob('pass-*.egg'))
            if not dumps:raise ValueError('production capture has no pass inputs')
            for dump in dumps:
                old=next((p for p in case['passes'] if p['name']==dump.stem),None)
                if old and old['status']=='complete':continue
                flat=folder/(dump.stem+'-flat.egg');compressed=folder/(dump.stem+'.jsonl.gz')
                if old and flat.exists():
                    if sha(flat)!=old['input_sha256']:raise ValueError('interrupted pass input changed; preserve it and use a new capture')
                else:subprocess.run([str(flatten),str(dump),str(flat)],check=True,cwd=repo)
                record=dict(name=dump.stem,input=str(flat),input_sha256=sha(flat),profile=str(compressed),status='running')
                if old:record['previous_attempts']=old.get('previous_attempts',[])+[{k:v for k,v in old.items() if k!='previous_attempts'}]
                partial=compressed.with_suffix('.partial.gz')
                if partial.exists():
                    attempt=1
                    while partial.with_name(partial.name+f'.attempt-{attempt}').exists():attempt+=1
                    saved=partial.with_name(partial.name+f'.attempt-{attempt}');partial.rename(saved)
                    record['previous_partial']=str(saved)
                if old is not None:case['passes'][case['passes'].index(old)]=record
                else:case['passes'].append(record)
                save(manifest,state)
                print(' ',dump.stem,'profiling',flush=True)
                command=[str(probe),str(flat),'-'];record['command']=command
                launch=['/usr/bin/time','-l',*command] if sys.platform=='darwin' else command
                record['launch']=launch;save(manifest,state);last=b'';lines=0
                with (folder/(dump.stem+'.log')).open('wb') as log, gzip.open(partial,'wb',compresslevel=1) as dest:
                    child=subprocess.Popen(launch,cwd=repo,stdout=subprocess.PIPE,stderr=log)
                    try:
                        for line in child.stdout:dest.write(line);last=line;lines+=1
                        code=child.wait()
                    except BaseException:
                        child.terminate();child.wait();raise
                record['returncode']=code
                if code or not last or json.loads(last).get('complete') is not True:
                    record['status']='failed';raise ValueError(f'{dump.stem} profile failed (exit {code}); partial counts excluded; see resource log')
                partial.replace(compressed);record.update(status='complete',profile_sha256=sha(compressed),records=lines)
                save(manifest,state)
                print(' ',dump.stem,'complete;',lines,'count records',flush=True)
            case['status']='complete'
        except Exception as e:
            case.update(status='failed',error=str(e));print('  FAILED:',e,flush=True)
        save(manifest,state)
    state['complete']=len(state['cases'])==len(sources) and all(c['status']=='complete' for c in state['cases'])
    state['finished']=len(state['cases'])==len(sources) and all(c['status'] in ('complete','failed') for c in state['cases'])
    save(manifest,state)
    print('Corpus:',sum(c['status']=='complete' for c in state['cases']),'complete,',sum(c['status']=='failed' for c in state['cases']),'failed;',manifest)
    return 0 if state['complete'] else 1
