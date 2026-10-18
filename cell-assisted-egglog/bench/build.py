"""Reproducible offline release builds; never modify the baseline checkout."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
try:
    import tomllib
except ImportError:
    raise SystemExit('Build helper requires Python 3.11+ (for Cargo.lock verification).')

ROOT=Path(__file__).resolve().parents[2]
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()

def write_changed(path,data):
    if isinstance(data,str):data=data.encode()
    if not path.exists() or path.read_bytes()!=data:path.write_bytes(data)

def git(*args,cwd=ROOT):return subprocess.check_output(['git',*args],cwd=cwd,text=True).strip()

def build(baseline, output):
    baseline=baseline.resolve();output.mkdir(parents=True,exist_ok=True)
    expected=git('rev-parse','egglog-baseline^{}')
    if git('rev-parse','HEAD',cwd=baseline)!=expected:raise ValueError('baseline checkout is not egglog-baseline')
    if git('status','--porcelain','--','egglog',cwd=baseline):raise ValueError('baseline kernel has local changes')
    pins={(p['name'],p['version'],p.get('source')) for p in tomllib.loads((ROOT/'cell-assisted-egglog/Cargo.lock').read_text())['package'] if 'source' in p}
    commands=[];binaries={}
    def run(cmd):
        commands.append(list(map(str,cmd)))
        subprocess.run(cmd,cwd=ROOT,check=True)
    run(['cargo','build','--release','--offline','--manifest-path','cell-assisted-egglog/Cargo.toml','--target-dir','target/cell-assisted'])
    binaries['assisted']=ROOT/'target/cell-assisted/release/cell-assisted-egglog'
    run(['swiftc','-O',ROOT/'cell-assisted-egglog/matrix.swift','-o',output/'matrix-metal'])
    binaries['worker']=output/'matrix-metal'
    for name,source in [('pristine',baseline),('fork',ROOT)]:
        package=output/name;(package/'src').mkdir(parents=True,exist_ok=True)
        write_changed(package/'src/main.rs',(ROOT/'cell-assisted-egglog/bench/native.rs').read_bytes())
        write_changed(package/'src/public_snapshot.rs',(ROOT/'cell-assisted-egglog/src/public_snapshot.rs').read_bytes())
        write_changed(package/'build.rs','fn main() {}\n')
        write_changed(package/'Cargo.toml',f'''[package]
name="cell_native_bench"
version="0.1.0"
edition="2024"
[workspace]
[dependencies]
egglog={{path={json.dumps(str(source/'egglog'))},default-features=false}}
serde_json="1"
[build-dependencies]
syn={{version="2",features=["full"]}}
[profile.release]
lto="thin"
codegen-units=1
''')
        if not (package/'Cargo.lock').exists():shutil.copyfile(ROOT/'cell-assisted-egglog/Cargo.lock',package/'Cargo.lock')
        target=ROOT/'target'/f'cell-bench-{name}'
        run(['cargo','build','--release','--offline','--manifest-path',package/'Cargo.toml','--target-dir',target])
        resolved={(p['name'],p['version'],p.get('source')) for p in tomllib.loads((package/'Cargo.lock').read_text())['package'] if 'source' in p}
        if not resolved<=pins:raise ValueError(f'{name}: registry dependencies drifted from assistant lock')
        dest=output/f'native-{name}';shutil.copyfile(target/'release/cell_native_bench',dest);dest.chmod(0o755)
        binaries[name]=dest
    manifest=dict(baseline_commit=expected,baseline_tree_clean=True,profile='release ThinLTO codegen-units=1; System allocator',
        registry_versions_pinned=True,commands=commands,
        binaries={name:dict(path=str(path.resolve()),sha256=sha(path)) for name,path in binaries.items()},
        sources={str(p.relative_to(ROOT)):sha(p) for p in [*sorted((ROOT/'cell-assisted-egglog/src').glob('*.rs')),
            *[ROOT/'cell-assisted-egglog'/n for n in ['bench/native.rs','bench/build.py','matrix.swift','matrix.metal','numeric.metal','integer.metal','tables.metal','family.metal','layout_solver.py','Cargo.toml','Cargo.lock']],ROOT/'tools/cell_spine/solver.py']},
        source_head=git('rev-parse','HEAD'),rustc=subprocess.check_output(['rustc','-Vv'],text=True))
    (output/'build.json').write_text(json.dumps(manifest,indent=2)+'\n')
    return manifest

if __name__=='__main__':
    import argparse
    p=argparse.ArgumentParser();p.add_argument('--baseline',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();build(a.baseline,a.output.resolve())
