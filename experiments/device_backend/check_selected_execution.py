"""Compare selected programs with the actual eggcc interpreter (finite inputs)."""
import argparse, hashlib, json, os, shutil, subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent

def main():
    p = argparse.ArgumentParser()
    for name in ('eggcc', 'build', 'target', 'before', 'after', 'inputs', 'output'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--entry', default='main')
    p.add_argument('--timeout', type=float, default=300)
    p.add_argument('--library-path', type=Path, help='native link directory for eggcc dependencies, e.g. Homebrew CBC')
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=False)
    src = a.build.resolve() / 'src'; src.mkdir(parents=True, exist_ok=True)
    (src / 'main.rs').write_text((HERE / 'interpret_selected.rs').read_text())
    # Pin the transitive git dependencies to the same revisions as production.
    shutil.copyfile(a.eggcc / 'Cargo.lock', a.build / 'Cargo.lock')
    (a.build / 'Cargo.toml').write_text('''[package]
name="egglog-selected-vm"
version="0.1.0"
edition="2021"
[dependencies]
dag_in_context={path=''' + json.dumps(str(a.eggcc.resolve() / 'dag_in_context')) + '''}
egglog={git="https://github.com/egraphs-good/egglog-experimental",rev="08771f9",package="egglog-experimental"}
indexmap="2"
serde_json="1"
''')
    with (a.output / 'build.log').open('w') as log:
        command = ['cargo', 'rustc', '--offline', '--release', '--manifest-path', str(a.build / 'Cargo.toml')]
        if a.library_path: command += ['--', '-Lnative=' + str(a.library_path)]
        subprocess.run(command,
                       env=dict(os.environ, CARGO_TARGET_DIR=str(a.target.resolve())), stdout=log, stderr=log, check=True)
    binary = a.target.resolve() / 'release/egglog-selected-vm'
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    report = {'scope': 'Finite-input production eggcc interpreter; not graph equality or a universal proof',
              'eggcc_commit': subprocess.check_output(['git','-C',str(a.eggcc),'rev-parse','HEAD'],text=True).strip(),
              'interpreter_sha256': sha(a.eggcc / 'dag_in_context/src/interpreter.rs'),
              'converter_sha256': sha(a.eggcc / 'dag_in_context/src/from_egglog.rs'),
              'schema_sha256': sha(a.eggcc / 'dag_in_context/src/schema.rs'),
              'cargo_lock_sha256': sha(a.build / 'Cargo.lock'),
              'binary_sha256': sha(binary), 'inputs_sha256': sha(a.inputs), 'entry': a.entry}
    outputs = []
    for name, path in [('before', a.before), ('after', a.after)]:
        with (a.output / (name + '.json')).open('w') as out, (a.output / (name + '.log')).open('w') as err:
            try:
                result = subprocess.run([str(binary), str(path), a.entry, str(a.inputs)], stdout=out, stderr=err, timeout=a.timeout)
                code, error = result.returncode, None
            except subprocess.TimeoutExpired:
                code, error = None, 'interpreter timeout'
        report[name] = {'source':str(path.resolve()), 'source_sha256':sha(path), 'returncode':code, 'error':error}
        outputs.append(json.loads((a.output / (name + '.json')).read_text()) if code == 0 else None)
    report['equal'] = outputs[0] is not None and outputs[0] == outputs[1]
    report['input_count'] = len(json.loads(a.inputs.read_text()))
    (a.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    if not report['equal']: raise SystemExit(1)

if __name__ == '__main__': main()
