"""Build the native frontend and the Metal execution transport, offline."""
import argparse,hashlib,json,os,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--target',type=Path,required=True);p.add_argument('--release',action='store_true');p.add_argument('--frontend-only',action='store_true');p.add_argument('--worker-only',action='store_true');p.add_argument('--reuse-native-build',type=Path);a=p.parse_args();o=a.output.resolve();(o/'src').mkdir(parents=True,exist_ok=True)
 if a.worker_only:
  if a.frontend_only or a.reuse_native_build is None:p.error('--worker-only requires --reuse-native-build and excludes --frontend-only')
  import shutil
  old=json.loads((a.reuse_native_build/'build.json').read_text());native=a.reuse_native_build/'frontend'
  sha=lambda x:hashlib.sha256(x.read_bytes()).hexdigest()
  if sha(native)!=old['frontend_sha256']:raise ValueError('cached frontend fingerprint mismatch')
  shutil.copy2(native,o/'frontend')
  subprocess.run(['xcrun','swiftc','-O',str(HERE/'worker.swift'),'-o',str(o/'worker')],check=True)
  old.update(frontend=str(o/'frontend'),worker_sha256=sha(o/'worker'),reused_native_build=str(a.reuse_native_build.resolve()),git_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip())
  for name in ['compiler.py','batch_rhs.py','batch_rhs.metal','matrix_sat.py','matrix_sat.metal','matrix_comparison.py','matrix_comparison.metal','query_contexts.py','context_ridge.py','contexts.metal','worker.swift','kernel.metal','sets.metal','actions.metal','row_index.metal','query_index.metal','rebuild_parallel.metal','query_parallel.metal','match_sort.metal','run.py']:
   old['sources'][str(HERE/name)]=sha(HERE/name)
  (o/'build.json').write_text(json.dumps(old,indent=2));return
 (o/'Cargo.toml').write_text('[package]\nname="egglog-device-frontend"\nversion="0.1.0"\nedition="2024"\n[dependencies]\negglog={path='+json.dumps(str(ROOT/'egglog'))+',default-features=false,features=["serde"]}\nserde_json="1"\n[profile.dev]\ndebug=0\nincremental=false\n')
 dest=o/'src/main.rs'
 if not dest.exists():dest.symlink_to(HERE/'frontend.rs')
 helper=o/'src/compact_observer.rs'
 if not helper.exists():helper.symlink_to(HERE/'compact_observer.rs')
 subprocess.run(['cargo','build','--offline','--manifest-path',str(o/'Cargo.toml')]+(['--release'] if a.release else []),env=dict(os.environ,CARGO_TARGET_DIR=str(a.target.resolve())),check=True)
 import shutil
 shutil.copy2(a.target/('release' if a.release else 'debug')/'egglog-device-frontend',o/'frontend')
 swift='/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc';sdk='/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk'
 if not a.frontend_only and (HERE/'worker.swift').exists():subprocess.run([swift,'-O','-sdk',sdk,str(HERE/'worker.swift'),'-o',str(o/'worker')],check=True)
 sha=lambda x:hashlib.sha256(x.read_bytes()).hexdigest()
 sources=[ROOT/'egglog/src/device_program.rs',ROOT/'egglog/egglog-bridge/src/device_import.rs',ROOT/'egglog/src/lib.rs',ROOT/'egglog/egglog-bridge/src/rule.rs',*[HERE/n for n in ['frontend.rs','compiler.py','batch_rhs.py','batch_rhs.metal','matrix_sat.py','matrix_sat.metal','matrix_comparison.py','matrix_comparison.metal','query_contexts.py','context_ridge.py','contexts.metal','worker.swift','kernel.metal','sets.metal','actions.metal','row_index.metal','query_index.metal','rebuild_parallel.metal','query_parallel.metal','match_sort.metal','run.py']]]
 tracked=subprocess.check_output(['git','ls-files','-z','egglog'],cwd=ROOT).decode().split('\0')
 kernel=sorted(set([n for n in tracked if n.endswith(('.rs','.toml','.lock'))]+['egglog/src/device_program.rs']))
 digest=hashlib.sha256()
 for name in kernel:
  f=ROOT/name
  if f.is_file():digest.update(name.encode()+b'\0'+sha(f).encode())
 sources.append(HERE/'compact_observer.rs')
 provenance={'profile':'release' if a.release else 'debug','sources':{str(x):sha(x) for x in sources},'native_kernel_source_sha256':digest.hexdigest(),'git_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'baseline_tag':subprocess.check_output(['git','rev-parse','egglog-baseline'],cwd=ROOT,text=True).strip(),'native_kernel_dirty':subprocess.check_output(['git','status','--short','--','egglog'],cwd=ROOT,text=True).splitlines(),'frontend':str(o/'frontend'),'frontend_sha256':sha(o/'frontend'),'worker_sha256':sha(o/'worker') if not a.frontend_only else None}
 (o/'build.json').write_text(json.dumps(provenance,indent=2))
if __name__=='__main__':main()
