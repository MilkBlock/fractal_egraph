#!/usr/bin/env python3
from pathlib import Path
import subprocess,json,shutil,argparse,tarfile,io,hashlib
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser();p.add_argument('--output',required=True);a=p.parse_args()
base=(ROOT/a.output).resolve();base.mkdir(parents=True,exist_ok=False)
archive=subprocess.check_output(['git','archive','egglog-baseline','egglog'],cwd=ROOT)
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:tar.extractall(base/'pristine-src',filter='data')
shutil.copytree(ROOT/'egglog',base/'current-src/egglog',ignore=shutil.ignore_patterns('target','.git','__pycache__'))
for kind in ['pristine','current']:
 subprocess.run(['python3',str(ROOT/'experiments/table_memory/instrument.py'),str(base/f'{kind}-src/egglog'),kind],check=True)
metadata={'baseline_commit':subprocess.check_output(['git','rev-parse','egglog-baseline^{}'],cwd=ROOT,text=True).strip(),'current_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'current_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),'instrument_sha256':hashlib.sha256((ROOT/'experiments/table_memory/instrument.py').read_bytes()).hexdigest()}
(base/'provenance.json').write_text(json.dumps(metadata,indent=2)+'\n')
for kind in ['pristine','current']:
 work=base/kind;(work/'src').mkdir(parents=True,exist_ok=True)
 shutil.copyfile(ROOT/'experiments/table_memory/main.rs',work/'src/main.rs')
 (work/'build.rs').write_text('fn main() {}\n')
 (work/'Cargo.toml').write_text('''[package]
name="table_memory"
version="0.1.0"
edition="2024"
[workspace]
[dependencies]
egglog={path=%s,default-features=false,features=["graphviz"]}
serde_json="1"
[build-dependencies]
syn={version="2",features=["full"]}
[features]
pack=[]
[profile.release]
lto="thin"
codegen-units=1
''' % json.dumps(str(base/f'{kind}-src/egglog')))
 shutil.copyfile(ROOT/'experiments/native_compare/Cargo.lock',work/'Cargo.lock')
 cmd=['cargo','build','--offline','--release','--manifest-path',str(work/'Cargo.toml'),'--target-dir',str(ROOT/'target/table-memory')]
 if kind=='current':cmd+=['--features','pack']
 subprocess.run(cmd,check=True)
 shutil.copyfile(ROOT/'target/table-memory/release/table_memory',base/f'{kind}-probe')
 (base/f'{kind}-probe').chmod(0o755)
