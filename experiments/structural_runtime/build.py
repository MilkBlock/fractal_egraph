"""Build against the existing assisted runtime without changing its worktree.

Generated manifests/symlinks/binaries live in out; the actual runtime and bench
helpers are reused, not copied into this repository or silently reimplemented.
"""
import argparse,hashlib,json,os,subprocess,shutil
from pathlib import Path
HERE=Path(__file__).resolve().parent
def main():
    p=argparse.ArgumentParser();p.add_argument('--runtime-repo',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--shared-rows',action='store_true',help='share immutable fact rows across projections and answer snapshots')
    p.add_argument('--query-worker',action='store_true',help='also build the existing full GPU matcher')
    a=p.parse_args();repo=a.runtime_repo.resolve();out=a.output.resolve();out.mkdir(parents=True,exist_ok=True)
    manifest='[package]\nname="structural-runtime"\nversion="0.1.0"\nedition="2024"\n[dependencies]\n'
    for name,rel in [('egglog','egglog'),('egglog-reports','egglog/egglog-reports'),('egglog-numeric-id','egglog/numeric-id'),('cell-assisted-egglog','cell-assisted-egglog')]:
        feature=',features=["serde"]' if name=='egglog' else ''
        manifest+=f'{name}={{path={json.dumps(str(repo/rel))},default-features=false{feature}}}\n'
    manifest+='serde={version="1",features=["derive"]}\nserde_json="1"\nrustc-hash="2"\n[features]\nshared-rows=[]\n[profile.release]\nlto="thin"\ncodegen-units=1\n'
    (out/'Cargo.toml').write_text(manifest);(out/'src').mkdir(exist_ok=True)
    for name,path in [('main.rs',HERE/'main.rs'),('hook.rs',HERE/'hook.rs'),('coordinates.rs',HERE/'coordinates.rs'),('markers.rs',HERE/'markers.rs'),('summaries.rs',HERE/'summaries.rs'),('conditions.rs',HERE/'conditions.rs'),('bench.rs',repo/'examples/support/live_table_bench.rs')]:
        dst=out/'src'/name
        if not dst.exists():dst.symlink_to(path)
        elif dst.resolve()!=path.resolve():raise ValueError(f'wrong existing build link {dst}')
    source=(HERE.parent/'superposed_key_memory/gpu.swift').read_text().split('let source="""\n',1)[1].split('\n"""',1)[0]
    (out/'kernel.metal').write_text(source+'\n'+(HERE/'shared_matrix.metal').read_text())
    swift='/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc'
    sdk='/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk'
    subprocess.run([swift,'-O','-sdk',sdk,str(HERE/'runtime.swift'),'-o',str(out/'worker')],check=True)
    if a.query_worker:subprocess.run([swift,'-O','-sdk',sdk,str(repo/'cell-assisted-egglog/matrix.swift'),'-o',str(out/'query-worker')],check=True)
    env=os.environ.copy();env['CARGO_TARGET_DIR']=str(repo/'target/analyzer')
    subprocess.run(['cargo','build','--offline','--release','--manifest-path',str(out/'Cargo.toml')]+(['--features','shared-rows'] if a.shared_rows else []),env=env,check=True)
    # Pin the actual executable to this build so feature variants cannot overwrite each other.
    shutil.copy2(repo/'target/analyzer/release/structural-runtime',out/'runner')
    sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    provenance={'runtime_repo':str(repo),'runtime_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),
        'runtime_dirty':subprocess.check_output(['git','status','--short'],cwd=repo,text=True).splitlines(),
        'binary':str(out/'runner'),'features':['shared-rows'] if a.shared_rows else [],
        'binary_sha256':sha(out/'runner'),
        'worker_sha256':sha(out/'worker'),'kernel_sha256':sha(out/'kernel.metal'),
        'sources':{str(p):sha(p) for p in [HERE/'hook.rs',HERE/'coordinates.rs',HERE/'markers.rs',HERE/'summaries.rs',HERE/'conditions.rs',HERE/'main.rs',HERE/'runtime.swift',HERE/'shared_matrix.metal',repo/'examples/support/live_table_bench.rs']}}
    if a.query_worker:
        provenance['ordinary_query_worker_sha256']=sha(out/'query-worker')
        provenance['ordinary_query_worker_source_sha256']=sha(repo/'cell-assisted-egglog/matrix.swift')
    (out/'build.json').write_text(json.dumps(provenance,indent=2)+'\n')
    print(json.dumps(provenance,indent=2))
if __name__=='__main__':main()
