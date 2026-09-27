#!/usr/bin/env python3
"""Build identical standalone consumers of the immutable baseline and current engine."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--baseline', type=Path, required=True, help='Clean checkout of egglog-baseline')
a = p.parse_args()
baseline = a.baseline.resolve()
expected = subprocess.check_output(['git', 'rev-parse', 'egglog-baseline'], cwd=ROOT).strip()
actual = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=baseline).strip()
assert actual == expected, 'Baseline must be checked out at egglog-baseline'
assert not subprocess.check_output(['git', 'status', '--porcelain', '--', 'egglog'], cwd=baseline), 'Baseline source must be clean'
base = ROOT / 'out/native-compare'
(base / 'bin').mkdir(parents=True, exist_ok=True)
for name, source, target, label in [('current', ROOT, 'native-compare', 'after'), ('baseline', baseline, 'native-compare-baseline', 'pristine')]:
    work = base / name
    (work / 'src').mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / 'experiments/native_compare/main.rs', work / 'src/main.rs')
    (work / 'build.rs').write_text('fn main() {}\n')
    (work / 'Cargo.toml').write_text('''[package]
name="native_compare"
version="0.1.0"
edition="2024"
[workspace]
[dependencies]
egglog={path=%s,default-features=false,features=["graphviz"]}
egglog-numeric-id={path=%s}
serde_json="1"
mimalloc={version="0.1",optional=true}
[profile.release]
lto="thin"
codegen-units=1
[build-dependencies]
syn={version="2",features=["full"]}
[features]
mi=["dep:mimalloc"]
''' % (json.dumps(str(source/'egglog')), json.dumps(str(source/'egglog/numeric-id'))))
    shutil.copyfile(ROOT / 'experiments/native_compare/Cargo.lock', work / 'Cargo.lock')
    for features, suffix in [([], ''), (['--features', 'mi'], '-mi')]:
        subprocess.run(['cargo', 'build', '--release', '--offline', '--locked', '--manifest-path', str(work/'Cargo.toml'), '--target-dir', str(ROOT/'target'/target), *features], check=True)
        shutil.copyfile(ROOT/'target'/target/'release/native_compare', base/'bin'/(label+suffix))
        (base/'bin'/(label+suffix)).chmod(0o755)
