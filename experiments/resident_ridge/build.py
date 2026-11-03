"""Isolated adapter to the existing native whole-Cell certificate runtime."""
import argparse,hashlib,json,os,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent

def build(repo,out):
    repo=repo.resolve();out=out.resolve();(out/'src').mkdir(parents=True,exist_ok=True)
    manifest='[package]\nname="resident-ridge-oracle"\nversion="0.1.0"\nedition="2024"\n[dependencies]\n'
    manifest+=f'egg_layout={{path={json.dumps(str(repo))}}}\negglog={{path={json.dumps(str(repo/"egglog"))},default-features=false}}\nserde_json="1"\n[profile.release]\nlto="thin"\ncodegen-units=1\n'
    (out/'Cargo.toml').write_text(manifest)
    dst=out/'src/main.rs'
    if not dst.exists():dst.symlink_to(HERE/'oracle.rs')
    elif dst.resolve()!=HERE/'oracle.rs':raise ValueError('wrong build link')
    env=dict(os.environ,CARGO_TARGET_DIR=str(repo/'target/analyzer'))
    subprocess.run(['cargo','build','--offline','--release','--manifest-path',str(out/'Cargo.toml')],check=True,env=env)
    swift='/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc'
    sdk='/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk'
    subprocess.run([swift,'-O','-sdk',sdk,str(HERE/'worker.swift'),'-o',str(out/'worker')],check=True)
    binary=repo/'target/analyzer/release/resident-ridge-oracle'
    info={'binary':str(binary),'runtime_repo':str(repo),'runtime_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),
          'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'oracle_sha256':hashlib.sha256((HERE/'oracle.rs').read_bytes()).hexdigest(),
          'worker_sha256':hashlib.sha256((out/'worker').read_bytes()).hexdigest(),'worker_source_sha256':hashlib.sha256((HERE/'worker.swift').read_bytes()).hexdigest(),'kernel_sha256':hashlib.sha256((HERE/'kernel.metal').read_bytes()).hexdigest()}
    (out/'build.json').write_text(json.dumps(info,indent=2)+'\n');return binary

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--runtime-repo',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();print(build(a.runtime_repo,a.output))
