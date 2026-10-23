"""Source/backend isolation with a verified immutable upstream runner.

No stable-choice rewrite or Cell observer is installed in these lanes. Cache
identity includes source, executables, shared harness, worker and run protocol.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import tomllib

BASELINE=Path('/Users/mineralsteins/.codex/worktrees/native-baseline/egg_layout')

def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def write(p,v):Path(p).write_text(json.dumps(v,indent=2)+'\n')
def git(repo,*args):return subprocess.check_output(['git',*args],cwd=repo,text=True).strip()


def build_pristine(repo,checkout,no_build=False):
    kernel=git(repo,'rev-parse','egglog-baseline:egglog')
    if git(checkout,'rev-parse','HEAD:egglog')!=kernel or git(checkout,'status','--porcelain','--','egglog'):
        raise ValueError('pristine checkout kernel differs from immutable egglog-baseline or is dirty')
    # Untracked Rust sources can participate in a path dependency too.
    if git(checkout,'ls-files','--others','--exclude-standard','--','egglog'):
        raise ValueError('untracked files in pristine kernel')
    folder=repo/'out/program-modes/pristine-build';binary=repo/'out/program-modes/pristine'
    harness=repo/'cell-assisted-egglog/src/program_io.rs';lock=repo/'experiments/native_compare/Cargo.lock'
    identity=dict(kernel_tree=kernel,baseline_commit=git(repo,'rev-parse','egglog-baseline'),harness_sha256=sha(harness),lock_sha256=sha(lock),protocol='program-run/v1')
    metadata=binary.with_suffix('.json')
    if metadata.exists() and binary.exists():
        old=json.loads(metadata.read_text())
        if all(old.get(k)==v for k,v in identity.items()) and old.get('binary_sha256')==sha(binary):return binary,old
    if no_build:raise ValueError('no current verified pristine runner; omit --no-build once')
    (folder/'src').mkdir(parents=True,exist_ok=True)
    (folder/'src/main.rs').write_text(f'''#[path={json.dumps(str(harness))}] mod program_io;
fn main()->program_io::Result<()> {{
 let a=std::env::args().skip(1).collect::<Vec<_>>();
 if a.len()!=2 {{return Err("SOURCE OUTPUT".into())}}
 egglog::EGraph::set_num_threads(1);
 program_io::native(std::path::Path::new(&a[0]),std::path::Path::new(&a[1]),"pristine",
 serde_json::json!({{"kernel_tree":{json.dumps(kernel)},"baseline_commit":{json.dumps(identity['baseline_commit'])},"source":"immutable egglog-baseline kernel"}}))
}}
''')
    (folder/'Cargo.toml').write_text('[package]\nname="cell_pristine_program"\nversion="0.1.0"\nedition="2024"\n[workspace]\n[dependencies]\n'
        +f'egglog={{path={json.dumps(str(checkout/"egglog"))},default-features=false}}\nserde_json="1"\n'
        +'[profile.release]\nlto="thin"\ncodegen-units=1\n[build-dependencies]\nsyn={version="2",features=["full"]}\n')
    (folder/'build.rs').write_text('fn main() {}\n');shutil.copyfile(lock,folder/'Cargo.lock')
    target=repo/'target/program-pristine'
    subprocess.run(['cargo','build','--offline','--release','--manifest-path',str(folder/'Cargo.toml'),'--target-dir',str(target)],cwd=repo,check=True)
    pins=lambda p:{(x['name'],x['version'],x.get('source')) for x in tomllib.loads(p.read_text())['package'] if 'source' in x}
    if not pins(folder/'Cargo.lock')<=pins(lock):raise ValueError('pristine registry dependency versions drifted')
    shutil.copyfile(target/'release/cell_pristine_program',binary);binary.chmod(0o755)
    identity['binary_sha256']=sha(binary);identity['source_clean']=True;write(metadata,identity)
    return binary,identity


def compare(left,right,tables=None):
    """Exact public typed rows, function values, equality and subsumption.

    IDs may differ; establish a bijection from actual constructor/function rows.
    A purely ungrounded cycle is inconclusive rather than guessed equal.
    """
    names=set(left['tables']) if tables is None else set(tables)
    if tables is None and names!=set(right['tables']):return dict(status='different',reason='public table schemas differ')
    for n in sorted(names):
        if n not in right['tables'] or left['tables'][n]!=right['tables'][n]:return dict(status='different',reason='schema differs',table=n)
    if tables is not None:
        base={'i64','f64','bool','String','Unit'}
        original_sorts={s for n in names for s in left['tables'][n]['input']+[left['tables'][n]['output']]}-base
        unsafe=[n for n,s in right['tables'].items() if n not in names and s['output'] in original_sorts]
        if unsafe:return dict(status='inconclusive',reason='new tables can add alternatives to original sorts; explicit extraction/auxiliary contract required',tables=sorted(unsafe))
    def atom(v):return (v[0],next(iter(v[1])),next(iter(v[1].values())))
    def rows(g):return [(r[0],tuple(map(atom,r[1])),atom(r[2]),r[3]) for r in g['rows'] if r[0] in names]
    a,b=rows(left),rows(right)
    from collections import Counter
    ca,cb=Counter(r[0] for r in a),Counter(r[0] for r in b)
    if ca!=cb:return dict(status='different',reason='row counts differ',tables={n:[ca[n],cb[n]] for n in sorted(ca.keys()|cb.keys()) if ca[n]!=cb[n]})
    index={(n,args):(v,h) for n,args,v,h in b}
    if len(index)!=len(b):return dict(status='inconclusive',reason='duplicate canonical keys')
    mapping={};inverse={};pending=a
    while pending:
        later=[];progress=False
        for n,args,v,hidden in pending:
            if any(x[1]=='eq' and x not in mapping for x in args):later.append((n,args,v,hidden));continue
            key=(n,tuple(mapping.get(x,x) for x in args))
            if key not in index:return dict(status='different',reason='missing fact',table=n,arguments=key[1])
            value,visibility=index[key]
            if hidden!=visibility:return dict(status='different',reason='subsumption differs',table=n)
            if v[:2]!=value[:2]:return dict(status='different',reason='result type differs',table=n)
            if v[1]=='base':
                if v!=value:return dict(status='different',reason='function/base value differs',table=n,values=[v,value])
            else:
                if (v in mapping and mapping[v]!=value) or (value in inverse and inverse[value]!=v):return dict(status='different',reason='equality partition differs',table=n)
                mapping[v]=value;inverse[value]=v
            progress=True
        if not progress:return dict(status='inconclusive',reason='unmapped cycle or ungrounded class',remaining_rows=len(later))
        pending=later
    return dict(status='equal',exact_rows_values_equivalence_visibility=True,rows=len(a),mapped_classes=len(mapping),
        projected_out_left=sorted(set(left['tables'])-names),projected_out_right=sorted(set(right['tables'])-names))


def lane(repo,binary,source,backend,worker,pristine,cache,audit=False):
    identity=dict(source_sha256=sha(source),runner_sha256=sha(pristine) if backend=='pristine' else sha(binary),backend=backend,
        pristine_sha256=sha(pristine) if backend=='pristine' else None,
        worker_sha256=sha(worker) if backend=='gpu' else None,query_audit=bool(backend=='gpu' and audit),
        harness_sha256=sha(repo/'cell-assisted-egglog/src/program_io.rs'),threads=1,protocol='program-run/v1')
    # Native parser preflight: do not cache source programs that read external
    # files or whose output side effects must happen on every invocation.
    inspection=subprocess.check_output([str(binary),'--mode','program','--backend','inspect',
        '--source',str(source),'--output',str(cache/'unused-inspection')],cwd=repo,text=True)
    cache_safe=json.loads(inspection)['cache_safe']
    key=hashlib.sha256(json.dumps(identity,sort_keys=True).encode()).hexdigest()
    dest=cache/key
    candidates=[dest]
    if backend=='pristine':candidates+=sorted(cache.glob('*'))
    for saved in candidates if cache_safe else []:
        if not (saved/'identity.json').is_file() or not (saved/'report.json').is_file():continue
        old=json.loads((saved/'identity.json').read_text())
        # Older records also hashed the launcher. Its GPU implementation cannot
        # affect the separate, byte-identical pristine child process.
        if old.get('backend')=='pristine':old['runner_sha256']=old.get('pristine_sha256')
        if old!=identity:continue
        report=json.loads((saved/'report.json').read_text())
        if report.get('complete') and (not report.get('snapshot_complete') or (saved/'graph.json').exists()):return saved,report,True
    if dest.exists():
        # Preserve failed runs. Never make a failure a successful cached lane.
        dest=cache/(key+'-'+str(time.time_ns()))
    cache.mkdir(parents=True,exist_ok=True)
    log=dest.with_suffix('.log')
    cmd=[str(binary),'--mode','program','--backend',backend,'--source',str(source),'--output',str(dest)]
    if backend=='pristine':cmd+=['--pristine-runner',str(pristine)]
    if backend=='gpu':cmd+=['--gpu-worker',str(worker),'--query-audit','on' if audit else 'off']
    # Isolation lanes never inherit another task's observer/Bake/profile flags.
    env={k:v for k,v in os.environ.items() if not k.startswith(('CELL_','EGGLOG_'))}
    env.update(RAYON_NUM_THREADS='1',EGGLOG_PROBE_COUNTS_OFF='1')
    with log.open('w') as f:r=subprocess.run(cmd,cwd=repo,env=env,stdout=f,stderr=subprocess.STDOUT)
    dest.mkdir(parents=True,exist_ok=True)
    if not (dest/'report.json').exists():write(dest/'report.json',dict(complete=False,backend=backend,error='runner failed before report; see run.log'))
    write(dest/'identity.json',identity);write(dest/'command.json',dict(command=cmd,returncode=r.returncode,cache_safe=cache_safe))
    shutil.move(log,dest/'run.log')
    return dest,json.loads((dest/'report.json').read_text()),False


def run(a,repo):
    resolve=lambda p:p if p.is_absolute() else repo/p
    output=resolve(a.output)
    if output.exists():raise ValueError('use a new report directory; unchanged lanes are reused automatically')
    if a.dry_run:
        print('Build/verify pinned pristine runner and current executable; run only missing source/backend lanes; compare full public graphs.');return 0
    binary=repo/'target/analyzer/release/cell-assisted-egglog'
    backends=['pristine','gpu'] if a.backend=='both' else [a.backend]
    pristine=None;provenance=None
    if 'pristine' in backends:pristine,provenance=build_pristine(repo,resolve(a.pristine_checkout),a.no_build)
    if not a.no_build:
        subprocess.run(['cargo','build','--offline','--release','--manifest-path','cell-assisted-egglog/Cargo.toml','--target-dir','target/analyzer','--bin','cell-assisted-egglog'],cwd=repo,check=True)
    if 'gpu' in backends and not a.no_build:
        worker=resolve(a.worker);fingerprint=worker.with_name(worker.name+'.skill-build.json')
        shader=repo/'cell-assisted-egglog/matrix.swift'
        known=json.loads(fingerprint.read_text()) if fingerprint.is_file() else {}
        if not (worker.is_file() and known.get('source_sha256')==sha(shader) and known.get('binary_sha256')==sha(worker)):
            worker.parent.mkdir(parents=True,exist_ok=True)
            subprocess.run(['xcrun','swiftc','-O',str(shader),'-o',str(worker)],cwd=repo,check=True)
            write(fingerprint,dict(source_sha256=sha(shader),binary_sha256=sha(worker)))
    if not binary.is_file():raise ValueError('missing current program runner')
    if 'gpu' in backends and not resolve(a.worker).is_file():raise ValueError('missing GPU worker')
    sources={'original':resolve(a.source)}
    if a.marked:sources['marked']=resolve(a.marked)
    output.mkdir(parents=True);lanes={}
    for label,source in sources.items():
        for backend in backends:
            dest,report,reused=lane(repo,binary,source,backend,resolve(a.worker),pristine,repo/'out/program-modes/cache',a.query_audit)
            name=label+'-'+backend
            (output/name).symlink_to(dest,target_is_directory=True)
            lanes[name]=dict(directory=str(dest),reused=reused,source=str(source),source_sha256=sha(source),
                complete=report.get('complete',False),snapshot_complete=report.get('snapshot_complete',False),
                run_seconds=report.get('run_seconds'),error=report.get('error'))
    comparisons={}
    def check(label,l,r,project=False):
        if l not in lanes or r not in lanes:return
        if not all(lanes[n]['complete'] and lanes[n]['snapshot_complete'] for n in [l,r]):
            comparisons[label]=dict(status='inconclusive',reason='execution or snapshot failed',left=l,right=r);return
        left=json.loads((Path(lanes[l]['directory'])/'graph.json').read_text())
        right=json.loads((Path(lanes[r]['directory'])/'graph.json').read_text())
        comparisons[label]=dict(compare(left,right,set(left['tables']) if project else None),left=l,right=r)
    check('marker_effect_native','original-native','marked-native',True)
    check('marker_effect_pristine','original-pristine','marked-pristine',True)
    check('marker_effect_gpu','original-gpu','marked-gpu',True)
    check('backend_original','original-pristine','original-gpu')
    check('backend_marked','marked-pristine','marked-gpu')
    diagnosis=[]
    if comparisons.get('marker_effect_pristine',{}).get('status')=='different':
        diagnosis.append('The marker rewrite already changes the pristine result; this difference does not require the GPU backend.')
    if comparisons.get('backend_original',{}).get('status')=='different':
        diagnosis.append('A backend difference exists before marker insertion; do not attribute all marked-program differences to markers.')
    if comparisons.get('marker_effect_pristine',{}).get('status')=='equal' and comparisons.get('backend_marked',{}).get('status')=='different':
        diagnosis.append('The marker rewrite preserves this pristine result, but the marked GPU result differs; inspect backend ordering and query audit.')
    result=dict(schema='program-backend-matrix/v1',diagnosis=diagnosis,lanes=lanes,comparisons=comparisons,pristine=provenance,
        scope='original source policy, no automatic stable-choice rewrite or Cell assistance; source effects and backend effects reported separately',
        passed=all(v['complete'] for v in lanes.values()) and all(v['status']=='equal' for v in comparisons.values()))
    write(output/'report.json',result)
    lines=['# Source / backend comparison','',result['scope'],'','| Lane | Seconds | Reused | Complete |','|---|---:|---|---|']
    lines += [f'| {k} | {v["run_seconds"]} | {v["reused"]} | {v["complete"]} |' for k,v in lanes.items()]
    lines += ['','| Comparison | Status |','|---|---|']+[f'| {k} | {v["status"]} |' for k,v in comparisons.items()]
    lines += ['']+diagnosis
    lines += ['','Marker comparisons include every original public table, value, equality and visibility flag; additional tables are listed in JSON. Same-source backend comparisons include marker tables too.','A disagreement is not automatically a GPU bug: original ordered merges or finite schedules may expose different execution orders. Failed checks are retained.']
    (output/'README.md').write_text('\n'.join(lines)+'\n')
    print(json.dumps({k:v['status'] for k,v in comparisons.items()},ensure_ascii=False));print(output/'README.md')
    return 0 if result['passed'] else 1
