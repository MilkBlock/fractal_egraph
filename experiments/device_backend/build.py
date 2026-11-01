"""Build the native frontend and the Metal execution transport, offline."""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
METAL = ROOT / 'engine/metal'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def runtime_sources():
    """Actual compiler/worker inputs, never forwarding modules or symlinks."""
    sources = [HERE / n for n in ('build.py', 'run.py', 'run_generic_cells.py')]
    for directory in ('eggfuse/analyzer', 'eggfuse/compiler', 'eggfuse/ir', 'eggfuse/engine'):
        sources.extend(p for p in (ROOT / directory).rglob('*.py')
                       if not p.name.startswith('test_'))
    sources.extend(p for p in METAL.iterdir() if p.suffix in ('.metal', '.swift'))
    sources.append(ROOT / 'engine/structural-key/abi.json')
    return sorted(set(sources))


def refresh_runtime_provenance(old_sources):
    # Reusing a frontend must retain the fingerprints of the Rust it was built
    # from, not those of today's working tree. The worker is freshly compiled;
    # replace its old source list so deleted forwarding paths cannot linger.
    sources = {name: digest for name, digest in old_sources.items()
               if Path(name).suffix in ('.rs', '.toml', '.lock')}
    sources.update({str(path): sha(path) for path in runtime_sources()})
    return sources


def swift_source(output):
    source = METAL / 'worker.swift'
    text = source.read_text()
    companions = {'// JOINT_HOST': 'joint_polyspine.swift',
                  '// STAGE_HOST': 'stage_contexts.swift',
                  '// SCHEDULED_HOST': 'scheduled_matrix.swift'}
    for token, filename in companions.items():
        if token in text:
            text = text.replace(token, (METAL / filename).read_text())
    dest = output / 'worker.generated.swift'
    dest.write_text(text)
    return dest


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--target', type=Path, required=True)
    p.add_argument('--release', action='store_true')
    p.add_argument('--frontend-only', action='store_true')
    p.add_argument('--worker-only', action='store_true')
    p.add_argument('--reuse-native-build', type=Path)
    a = p.parse_args()
    o = a.output.resolve()
    (o / 'src').mkdir(parents=True, exist_ok=True)
    if a.worker_only:
        if a.frontend_only or a.reuse_native_build is None:
            p.error('--worker-only requires --reuse-native-build and excludes --frontend-only')
        old = json.loads((a.reuse_native_build / 'build.json').read_text())
        native = a.reuse_native_build / 'frontend'
        if sha(native) != old['frontend_sha256']:
            raise ValueError('cached frontend fingerprint mismatch')
        shutil.copy2(native, o / 'frontend')
        subprocess.run(['xcrun', 'swiftc', '-O', str(swift_source(o)),
                        '-o', str(o / 'worker')], check=True)
        old.update(frontend=str(o / 'frontend'), worker_sha256=sha(o / 'worker'),
                   reused_native_build=str(a.reuse_native_build.resolve()),
                   git_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                   sources=refresh_runtime_provenance(old['sources']))
        (o / 'build.json').write_text(json.dumps(old, indent=2))
        return

    (o / 'Cargo.toml').write_text(
        '[package]\nname="egglog-device-frontend"\nversion="0.1.0"\nedition="2024"\n'
        '[dependencies]\negglog={path=' + json.dumps(str(ROOT / 'egglog')) +
        ',default-features=false,features=["serde"]}\n'
        'eggfuse-structural-key={path=' + json.dumps(str(ROOT / 'engine/structural-key')) + '}\nserde_json="1"\n'
        '[profile.dev]\ndebug=0\nincremental=false\n')
    for name in ('main.rs', 'compact_observer.rs'):
        dest = o / 'src' / name
        if not dest.exists():
            dest.symlink_to(HERE / ('frontend.rs' if name == 'main.rs' else name))
    subprocess.run(['cargo', 'build', '--offline', '--manifest-path', str(o / 'Cargo.toml')]
                   + (['--release'] if a.release else []),
                   env=dict(os.environ, CARGO_TARGET_DIR=str(a.target.resolve())), check=True)
    shutil.copy2(a.target / ('release' if a.release else 'debug') / 'egglog-device-frontend', o / 'frontend')
    if not a.frontend_only:
        subprocess.run(['xcrun', 'swiftc', '-O', str(swift_source(o)),
                        '-o', str(o / 'worker')], check=True)
    sources = [ROOT / n for n in ('egglog/src/device_program.rs',
               'egglog/egglog-bridge/src/device_import.rs', 'egglog/src/lib.rs',
               'egglog/egglog-bridge/src/rule.rs')]
    sources.extend(HERE / n for n in ('frontend.rs', 'compact_observer.rs'))
    sources.extend((ROOT / 'src/structural_key').glob('*.rs'))
    sources.extend((ROOT / 'engine/structural-key/src').glob('*.rs'))
    sources.extend(ROOT / 'engine/structural-key' / n for n in ('Cargo.toml', 'Cargo.lock'))
    sources.extend(runtime_sources())
    tracked = subprocess.check_output(['git', 'ls-files', '-z', 'egglog'], cwd=ROOT).decode().split('\0')
    kernel = sorted(set([n for n in tracked if n.endswith(('.rs', '.toml', '.lock'))]
                        + ['egglog/src/device_program.rs']))
    digest = hashlib.sha256()
    for name in kernel:
        f = ROOT / name
        if f.is_file():
            digest.update(name.encode() + b'\0' + sha(f).encode())
    provenance = {
        'profile': 'release' if a.release else 'debug',
        'sources': {str(x): sha(x) for x in sources},
        'native_kernel_source_sha256': digest.hexdigest(),
        'git_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'baseline_tag': subprocess.check_output(['git', 'rev-parse', 'egglog-baseline'], cwd=ROOT, text=True).strip(),
        'native_kernel_dirty': subprocess.check_output(['git', 'status', '--short', '--', 'egglog'], cwd=ROOT, text=True).splitlines(),
        'frontend': str(o / 'frontend'), 'frontend_sha256': sha(o / 'frontend'),
        'worker_sha256': sha(o / 'worker') if not a.frontend_only else None,
    }
    (o / 'build.json').write_text(json.dumps(provenance, indent=2))


if __name__ == '__main__':
    main()
