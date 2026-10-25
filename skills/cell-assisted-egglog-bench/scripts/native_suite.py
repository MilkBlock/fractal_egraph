"""Native source-pipeline suite. Real pass/extraction chaining, no GPU.

Preparation may compile newly encountered pass inputs; timed candidate execution
must load the resulting artifacts or explicitly recorded complete-native fallback.
No final graphs or per-match traces are exported.
"""
import csv
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import signal
import subprocess
import threading
import time

def sha(path):
    h=hashlib.sha256()
    with Path(path).open('rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
    return h.hexdigest()

def load(p):return json.loads(Path(p).read_text())
def write(p,d):
    p=Path(p);p.parent.mkdir(parents=True,exist_ok=True);tmp=p.with_suffix(p.suffix+'.partial')
    tmp.write_text(json.dumps(d,indent=2)+'\n');tmp.replace(p)
def seconds(v):return v['secs']+v['nanos']*1e-9

def compact_results(out):
    """Drop duplicate definitions from summaries; retain every raw pass packet."""
    for file in (out/'cases').glob('*/result.json'):
        r=load(file)
        if r.get('compact_summary'):continue
        for lane in ['capture','prepare','before','after']:
            for n,p in r.get(lane,{}).get('passes',{}).items():
                evidence=file.parent/lane/'passes'/f'pass-{n}.json'
                if not evidence.is_file():continue
                p['evidence']=str(evidence)
                p['native']['runtime']={k:v for k,v in p['native']['runtime'].items() if not isinstance(v,(dict,list))}
                p['native']['counts'].pop('tables',None)
        r['compact_summary']=True
        temp=file.with_suffix('.compact');temp.write_text(json.dumps(r,indent=2)+'\n');temp.replace(file)

def execute(command,folder,cwd,env,timeout,identity):
    stamp=folder/'measurement.json'
    if stamp.exists():
        old=load(stamp)
        if old['identity']!=identity:raise ValueError('changed lane inputs: '+str(folder))
        if old.get('complete') and sha(folder/'stdout')==old['stdout_sha256']:
            return dict(old,reused=True)
        raise ValueError('incomplete lane retained: '+str(folder))
    folder.mkdir(parents=True,exist_ok=True)
    with (folder/'stdout').open('wb') as out,(folder/'stderr').open('wb') as err:
        start=time.perf_counter();p=subprocess.Popen(list(map(str,command)),cwd=cwd,env=env,
            stdout=out,stderr=err,start_new_session=True)
        expired=threading.Event()
        def stop():
            if p.poll() is None:
                expired.set()
                try:os.killpg(p.pid,signal.SIGKILL)
                except ProcessLookupError:pass
        timer=threading.Timer(timeout,stop);timer.daemon=True;timer.start()
        try:p.wait();wall=time.perf_counter()-start
        finally:timer.cancel();timer.join()
    r=dict(complete=True,returncode=p.returncode,timed_out=expired.is_set(),wall_seconds=wall,
        identity=identity,command=list(map(str,command)),stdout_sha256=sha(folder/'stdout'),
        measured_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
    write(stamp,r);return r

def summarize(out):
    manifest=load(out/'manifest.json');cases=[];passes=[]
    for item in manifest['workloads']:
        folder=out/'cases'/item['id'];file=folder/'result.json'
        if not file.exists():continue
        r=load(file);cases.append(r)
        for n in sorted(set(r.get('before',{}).get('passes',{}))|set(r.get('after',{}).get('passes',{})),key=int):
            a=r.get('before',{}).get('passes',{}).get(n);b=r.get('after',{}).get('passes',{}).get(n)
            row=dict(workload=r['source'],pass_number=int(n))
            if a and b:row['same_pass_input']=a['native']['source_cache']==b['native']['source_cache']
            for label,p in [('before',a),('after',b)]:
                if p:
                    native=p['native'];c=native['counts'];stats=native['runtime']
                    row.update({label+'_inference_s':native['execution_seconds'],
                        label+'_serialization_s':p['serialization_seconds'],label+'_extraction_s':p['extraction_seconds'],
                        label+'_counting_s':c['measurement_seconds'],label+'_fallback':native.get('native_fallback',False),
                        label+'_cell_bindings':stats.get('complete_cell_bindings',0),label+'_sat_skips':stats.get('skipped_saturations',0)})
                    for k in ['constructor_rows','visible_constructor_rows','constructor_eclasses','expr_enodes','expr_eclasses','auxiliary_rows']:
                        row[label+'_'+k]=c[k]
            for k in ['constructor_rows','visible_constructor_rows','constructor_eclasses','expr_enodes','expr_eclasses']:
                av=row.get('before_'+k);bv=row.get('after_'+k)
                row[k+'_delta']=None if av is None or bv is None else bv-av
                row[k+'_ratio']=None if not av or bv is None else bv/av
            if a and b:row['observed_inference_ratio']=row['before_inference_s']/row['after_inference_s']
            passes.append(row)
    paired=[r for r in cases if r.get('before',{}).get('returncode')==0 and r.get('after',{}).get('returncode')==0]
    def total(rows,lane,field):return sum(p['native']['execution_seconds'] if field=='inference' else p[field] for r in rows for p in r[lane]['passes'].values())
    b=total(paired,'before','inference');a=total(paired,'after','inference')
    summary=dict(schema='native-cell-suite/v1',requested=len(manifest['workloads']),completed=len(cases),paired_success=len(paired),
        before_inference_seconds=b,after_inference_seconds=a,observed_total_inference_ratio=b/a if a else None,
        identical_final_ir=sum(r.get('same_final_ir',False) for r in paired),
        cases=cases,passes=passes,scope='Real complete eggcc pass chains. Same patched native kernel, original source vs compiled Cell source; not pristine upstream. Tied winners may differ. Ratios are observations, not an independent semantic equivalence proof.')
    write(out/'summary.json',summary)
    def csvfile(name,rows):
        with (out/name).open('w',newline='') as f:
            fields=list(dict.fromkeys(k for r in rows for k in r));w=csv.DictWriter(f,fields);w.writeheader();w.writerows(rows)
    csvfile('passes.csv',passes)
    rows=[]
    for r in cases:
        row=dict(workload=r['source'],before_status=r.get('before',{}).get('returncode'),after_status=r.get('after',{}).get('returncode'),same_final_ir=r.get('same_final_ir'))
        row['preparation_wall_s']=r.get('prepare',{}).get('wall_seconds')
        for lane in ['before','after']:
            if lane in r:
                ps=r[lane]['passes'];row[lane+'_passes']=len(ps);row[lane+'_inference_s']=sum(p['native']['execution_seconds'] for p in ps.values())
                row[lane+'_wall_s']=r[lane].get('wall_seconds')
                row[lane+'_serialization_s']=sum(p['serialization_seconds'] for p in ps.values())
                row[lane+'_extraction_s']=sum(p['extraction_seconds'] for p in ps.values())
                if ps:
                    c=ps[max(ps,key=int)]['native']['counts']
                    row[lane+'_final_constructor_rows']=c['constructor_rows'];row[lane+'_final_eclasses']=c['constructor_eclasses']
        if row.get('before_status')==0 and row.get('after_status')==0:
            row['observed_inference_ratio']=row['before_inference_s']/row['after_inference_s']
            row['observed_wall_ratio']=row['before_wall_s']/row['after_wall_s'] if row['before_wall_s'] is not None and row['after_wall_s'] else None
            row['final_constructor_delta']=row['after_final_constructor_rows']-row['before_final_constructor_rows']
            row['final_eclass_delta']=row['after_final_eclasses']-row['before_final_eclasses']
        rows.append(row)
    csvfile('workloads.csv',rows)
    text=['# Native Cell suite','',summary['scope'],'',f"Completed {len(cases)}/{summary['requested']}; paired successful runs: {len(paired)}.",'',
        '| Workload | Before inference s | After inference s | Ratio | Final constructor delta | Final eclass delta | Same printed IR |',
        '|---|---:|---:|---:|---:|---:|---|']
    def fmt(v):return '' if v is None else f'{v:.4f}' if isinstance(v,float) else str(v)
    for r in rows:text.append('| '+' | '.join(fmt(r.get(k)) for k in ['workload','before_inference_s','after_inference_s','observed_inference_ratio','final_constructor_delta','final_eclass_delta','same_final_ir'])+' |')
    (out/'RESULTS.md').write_text('\n'.join(text)+'\n')
    return summary

def run(a,resolve):
    out=resolve(a.output)
    if a.report_only:print(summarize(out)['paired_success']);return 0
    checkout=resolve(a.checkout);fork=resolve(a.fork);python=resolve(a.python)
    sources=sorted(checkout/p for p in a.source) if a.source else sorted(p for p in (checkout/'benchmarks/passing').rglob('*') if p.suffix in ('.rs','.bril'))
    if a.dry_run:print('Workloads:',len(sources));return 0
    out.mkdir(parents=True,exist_ok=True);binary=checkout/'target/release/eggcc';tiger=binary.with_name('tiger')
    toolroot=fork/'cell-extension/framework/target/analyzer/release/examples'
    identity=dict(protocol='native-suite/v2-paired-warm',binary_sha256=sha(binary),tiger_sha256=sha(tiger),timeout=a.timeout,prepare_timeout=a.prepare_timeout,
        tools={n:sha(toolroot/n) for n in ['lhs_ast','structural_key_source','scheduled_spine','planner_profile']},
        compiler={str(p.relative_to(fork)):sha(p) for p in sorted((fork/'cell-extension/framework/tools/cell_spine').glob('*.py'))},
        pipeline=sha(fork/'cell-extension/pipeline.py'))
    workloads=[dict(id=f'{i:03d}-'+p.stem,source=str(p.relative_to(checkout)),sha256=sha(p)) for i,p in enumerate(sources)]
    manifest=dict(identity=identity,workloads=workloads)
    if (out/'manifest.json').exists() and load(out/'manifest.json')!=manifest:raise ValueError('changed suite identity; use a new directory')
    write(out/'manifest.json',manifest)
    frozen=out/'bin';frozen.mkdir(exist_ok=True)
    for src in [binary,tiger]:
        dst=frozen/src.name
        if not dst.exists():shutil.copy2(src,dst)
        if sha(src)!=sha(dst):raise ValueError('changed frozen binary')
    for i,item in enumerate(workloads):
        folder=out/'cases'/item['id'];folder.mkdir(parents=True,exist_ok=True)
        if (folder/'result.json').exists():continue
        print(f"[{i+1}/{len(workloads)}] {item['source']}",flush=True)
        r=dict(source=item['source'],source_sha256=item['sha256'])
        order=['capture','prepare']+(['before','after'] if i%2==0 else ['after','before'])
        for lane in order:
            if lane in ['before','after'] and r.get('prepare',{}).get('returncode')!=0:break
            dest=folder/lane
            env={k:v for k,v in os.environ.items() if not k.startswith(('CELL_','EGGLOG_','EGGCC_PASS_','TIGER_EXTRACTION_'))}
            env.update(RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',
                CELL_NATIVE_SUITE='1',CELL_NATIVE_MODE='record' if lane=='capture' else 'replay',
                CELL_NATIVE_STAGE='source' if lane in ['capture','before'] else 'mk',CELL_NATIVE_CACHE=str(out/'cache'),
                CELL_NATIVE_PASS_DIR=str(dest/'passes'))
            if lane=='prepare':env.update(CELL_NATIVE_AUTOCOMPILE=str(fork/'cell-extension/pipeline.py'),
                CELL_NATIVE_PYTHON=str(python),CELL_PROFILE_CACHE=str(out/'profiles'),CELL_NATIVE_COMPILE_FALLBACK='1')
            command=[frozen/'eggcc',checkout/item['source'],'--run-mode','optimized-pretty-print','--run-data-out',dest/'run.json']
            print(' ',lane,flush=True)
            measurement=execute(command,dest,checkout,env,a.prepare_timeout if lane=='prepare' else a.timeout,
                dict(suite=identity,source=item['sha256'],lane=lane))
            ps={str(load(p)['pass']):load(p) for p in sorted((dest/'passes').glob('pass-*.json'))}
            r[lane]=dict(returncode=measurement['returncode'],timed_out=measurement['timed_out'],
                wall_seconds=measurement['wall_seconds'],stdout_sha256=measurement['stdout_sha256'],passes=ps)
            print(' ',lane,measurement['returncode'],round(measurement['wall_seconds'],3),'s',flush=True)
            if lane=='capture' and measurement['returncode']!=0:break
        if r.get('after',{}).get('returncode')==0:
            r['same_final_ir']=r['before']['stdout_sha256']==r['after']['stdout_sha256']
            r['prepare_replay_same_ir']=r['prepare']['stdout_sha256']==r['after']['stdout_sha256']
            r['warm_artifacts_valid']=all(p['native']['cache_hit'] or p['native'].get('native_fallback') for p in r['after']['passes'].values())
        write(folder/'result.json',r);compact_results(out);summarize(out)
        # Preserve exact source artifacts, selected plans and compressed profiles;
        # discard regenerable expanded AST/inventory intermediates after replay.
        # No benchmark graphs or cached profiles are removed.
        keep={'source.egg','source_sk.egg','source_sk_mk.egg','pipeline.json',
              'markers/profile.jsonl.gz','markers/profile-manifest.json','markers/profile-cache.json',
              'markers/report.json','markers/selection.json','markers/selected.json','keys/report.json'}
        for cache in (out/'cache').iterdir():
            compiled=cache/'compiled'
            if not (compiled/'pipeline.json').exists() or (cache/'intermediates-pruned.json').exists():continue
            removed=0
            for p in compiled.rglob('*'):
                if p.is_file() and str(p.relative_to(compiled)) not in keep:
                    removed+=p.stat().st_size;p.unlink()
            write(cache/'intermediates-pruned.json',dict(regenerable_bytes_removed=removed,retained=sorted(keep)))
    summary=summarize(out)
    print('Completed:',summary['completed'],'paired:',summary['paired_success'],flush=True)
    return 0
