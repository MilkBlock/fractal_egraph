#!/usr/bin/env python3
"""Run the production Tiger on saved full graphs; check reconstructed IR."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import shutil
import time

ROOT = Path(__file__).resolve().parents[2]

def sha(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('run', type=Path)
    p.add_argument('--tiger', type=Path, default=Path('/Users/mineralsteins/Repos/egg_related/check_statewalk/target/release/tiger'))
    p.add_argument('--reference-output', type=Path)
    a = p.parse_args()
    output = a.run / 'extraction'
    output.mkdir(exist_ok=False)
    records = []
    report_path = a.run / 'report.json'
    report = json.loads(report_path.read_text()) if report_path.is_file() else {}
    old = report.get('reused_controls', {}).get('directory')
    old = Path(old) / 'extraction' if old else None
    timing = json.loads((old / 'timing.json').read_text()) if old and (old / 'timing.json').is_file() else {}
    tiger_hash = sha(a.tiger)
    for lane in ['native', 'cpu', 'gpu']:
        graph = a.run / 'graphs' / (lane + '.json')
        target = output / (lane + '.egg')
        cached = next((r for r in timing.get('runs', []) if r['lane'] == lane), None)
        if lane != 'gpu' and cached and timing.get('tiger_sha256') == tiger_hash and cached.get('returncode') == 0 and cached.get('input_sha256') == sha(graph) and (old / (lane + '.egg')).is_file() and cached.get('output_sha256') == sha(old / (lane + '.egg')):
            shutil.copyfile(old / (lane + '.egg'), target)
            records.append(dict(cached, reused_from=str(old)))
            (output / 'timing.json').write_text(json.dumps(dict(tiger=str(a.tiger), tiger_sha256=tiger_hash, runs=records), indent=2) + '\n')
            continue
        # Match production's serde_json::to_string_pretty contract. Its C++
        # reader tokenizes whitespace, so compact JSON is not accepted.
        pretty = output / (lane + '.input-pretty.json')
        with graph.open() as inp:
            data = json.load(inp)
        with pretty.open('w') as f:
            json.dump(data, f, indent=2)
            f.write('\n')
        del data
        with pretty.open('rb') as inp, target.open('wb') as out, (output / (lane + '.stderr')).open('wb') as err:
            start = time.perf_counter()
            result = subprocess.run([str(a.tiger), '--ilp-solver', 'cbc'], stdin=inp, stdout=out, stderr=err)
            seconds = time.perf_counter() - start
        records.append(dict(lane=lane, seconds=seconds, returncode=result.returncode,
                            input_sha256=sha(graph), output_sha256=sha(target)))
        (output / 'timing.json').write_text(json.dumps(dict(tiger=str(a.tiger), tiger_sha256=sha(a.tiger), runs=records), indent=2) + '\n')
        result.check_returncode()
        pretty.unlink()
    checker = ROOT / 'target/analyzer/release/examples/tiger_output_check'
    command = [str(checker), str(output / 'quality.json'), str(output / 'native.egg'),
               str(output / 'cpu.egg'), str(output / 'gpu.egg')]
    if a.reference_output:
        command.append(str(a.reference_output))
    subprocess.run(command, check=True)
    print(json.dumps(records, indent=2))
    print(output / 'quality.json')

if __name__ == '__main__':
    main()
