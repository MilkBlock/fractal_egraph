"""Production Tiger marker-cost ablation. Reuses a saved graph; no inference rerun."""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time


def sha(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as f:
        for b in iter(lambda: f.read(1024*1024), b''): h.update(b)
    return h.hexdigest()


def cached_lane(repo, identity, mode):
    """Newest fully checked identical run, never a fastest-run selection."""
    paths=sorted((repo/'out').glob('*/results.json'),key=lambda p:p.stat().st_mtime,reverse=True)
    for path in paths:
        try:
            old=json.loads(path.read_text())
            keys=['schema','input_sha256','tiger_sha256','machine']+(['worker_sha256'] if mode=='gpu' else [])
            if any(old.get(k)!=identity.get(k) for k in keys) or not old.get('quality',{}).get('passed'):continue
            row=next((r for r in old['runs'] if r['mode']==mode and r['returncode']==0 and r['timing_eligible']),None)
            output=path.parent/(mode+'.egg');err=path.parent/(mode+'.stderr')
            if row and output.is_file() and err.is_file() and sha(output)==row['output_sha256']:
                return path.parent,row
        except (OSError,ValueError,KeyError,TypeError):continue
    return None


def run(a, repo):
    sys.path.insert(0,str(repo))
    from tools.tiger_markers.build import build, HERE
    resolve=lambda p: p if p.is_absolute() else repo/p
    out=resolve(a.output); out.mkdir(parents=True,exist_ok=False)
    binary,worker=build(resolve(a.tiger_checkout),resolve(a.build_dir))
    source=binary.parent/'source'; test=binary.parent/'test-costs'
    subprocess.run(['c++','-std=c++17','-O2','-Wno-unused-result','-I',str(source),str(HERE/'test_costs.cpp'),
                    str(source/'greedy.cpp'),str(source/'egraphin.cpp'),'-o',str(test)],check=True)
    with (out/'tests.log').open('w') as f:
        subprocess.run([str(test),str(worker)],stdout=f,stderr=subprocess.STDOUT,check=True)
    graph=resolve(a.graph)
    # Exactly the existing production input serialization, retained just for this
    # extraction run. No marker table is added to the semantic input graph.
    pretty=out/'input.json'
    with graph.open() as f: data=json.load(f)
    with pretty.open('w') as f: json.dump(data,f,indent=2);f.write('\n')
    del data
    runs=[]
    manifest=dict(schema='tiger-extraction-markers/v1',graph=str(graph),input_sha256=sha(graph),
        tiger_sha256=sha(binary),worker_sha256=sha(worker),machine=platform.node(),runs=runs,
        scope='Actual production Tiger with optional shared sorted cost bags and cost recipes. Original greedy choices, sharing semantics, Statewalk and tie order retained. Not existing matching-marker/RipSat closure reuse or online extraction.')
    def execute(mode,verify=False):
        name=mode+('-verify' if verify else '')
        previous=None if verify else cached_lane(repo,manifest,mode)
        if previous:
            import shutil
            folder,row=previous
            for suffix in ('.egg','.stderr'):shutil.copyfile(folder/(mode+suffix),out/(mode+suffix))
            runs.append(dict(row,reused_from=str(folder)))
            return
        env={k:v for k,v in os.environ.items() if not k.startswith('TIGER_EXTRACTION_')}
        env.update(TIGER_EXTRACTION_MARKERS=mode,TIGER_EXTRACTION_WORKER=str(worker),TIGER_EXTRACTION_VERIFY='1' if verify else '0')
        with pretty.open('rb') as inp,(out/(name+'.egg')).open('wb') as result,(out/(name+'.stderr')).open('wb') as err:
            start=time.perf_counter(); p=subprocess.run([str(binary),'--ilp-solver','cbc'],stdin=inp,stdout=result,stderr=err,env=env)
            seconds=time.perf_counter()-start
        stats=[]
        for line in (out/(name+'.stderr')).read_text().splitlines():
            if line.startswith(('TIGER_MARKERS ','TIGER_MARKER_GPU ','TIGER_COST_PHASE ','TIGER_COST_BAG ','TIGER_GREEDY_ORACLE ')):stats.append(json.loads(line.split(' ',1)[1]))
        runs.append(dict(mode=name,seconds=seconds,timing_eligible=not verify,returncode=p.returncode,
                         output_sha256=sha(out/(name+'.egg')),stats=stats))
        (out/'results.json').write_text(json.dumps(manifest,indent=2)+'\n')
        p.check_returncode()
    for mode in ('off','bag','cpu','gpu'):execute(mode)
    execute('gpu',True) # checks every candidate against native; separate from timing
    checker=repo/'target/analyzer/release/examples/tiger_output_check'
    outputs=[out/(x+'.egg') for x in ('off','bag','cpu','gpu','gpu-verify')]
    if a.reference_output: outputs.append(resolve(a.reference_output))
    subprocess.run([str(checker),str(out/'quality.json'),*map(str,outputs)],check=True)
    manifest['quality']=json.loads((out/'quality.json').read_text())
    manifest['gpu_repeat_byte_identical']=sha(out/'gpu.egg')==sha(out/'gpu-verify.egg')
    manifest['all_modes_byte_identical']=len({sha(p) for p in outputs[:5]})==1
    manifest['bag_speedup']=runs[0]['seconds']/runs[1]['seconds']
    manifest['cpu_speedup']=runs[0]['seconds']/runs[2]['seconds']
    manifest['gpu_speedup']=runs[0]['seconds']/runs[3]['seconds']
    (out/'results.json').write_text(json.dumps(manifest,indent=2)+'\n')
    if not manifest['gpu_repeat_byte_identical']:raise ValueError('GPU extraction replay differed')
    pretty.unlink()
    print(json.dumps(manifest,indent=2))
