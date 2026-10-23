#!/usr/bin/env python3
"""Run the production Tiger on saved full graphs; check reconstructed IR."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import shutil
import time
import os
import sys

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
    p.add_argument('--markers', choices=['off','bag','cpu','gpu'], default='off')
    p.add_argument('--verify-markers', action='store_true')
    a = p.parse_args()
    output = a.run / 'extraction'
    output.mkdir(exist_ok=False)
    records = []
    report_path = a.run / 'report.json'
    report = json.loads(report_path.read_text()) if report_path.is_file() else {}
    old = report.get('reused_controls', {}).get('directory')
    old = Path(old) / 'extraction' if old else None
    timing = json.loads((old / 'timing.json').read_text()) if old and (old / 'timing.json').is_file() else {}
    worker = None
    if a.markers != 'off':
        sys.path.insert(0,str(ROOT))
        from tools.tiger_markers.build import ensure
        a.tiger, worker = ensure(Path('/Users/mineralsteins/Repos/egg_related/check_statewalk'),ROOT/'out/tiger-marker-tools')
    tiger_hash = sha(a.tiger)
    worker_hash = sha(worker) if worker else None
    config = dict(tiger=str(a.tiger),tiger_sha256=tiger_hash,marker_mode=a.markers,
                  marker_worker_sha256=worker_hash,marker_verification=a.verify_markers)
    env = {k:v for k,v in os.environ.items() if not k.startswith('TIGER_EXTRACTION_')}
    env.update(TIGER_EXTRACTION_MARKERS=a.markers,TIGER_EXTRACTION_VERIFY='1' if a.verify_markers else '0')
    if worker:env['TIGER_EXTRACTION_WORKER']=str(worker)
    lanes = report.get('extraction_lanes', ['native', 'cpu', 'gpu'])
    if lanes not in (['native','cpu','gpu'],['native','markers']):raise ValueError('unsupported extraction lanes')
    for lane in lanes:
        graph = a.run / 'graphs' / (lane + '.json')
        target = output / (lane + '.egg')
        cached = next((r for r in timing.get('runs', []) if r['lane'] == lane), None)
        same_mode = timing.get('marker_mode','off') == a.markers and timing.get('marker_worker_sha256') == worker_hash and timing.get('marker_verification',False) == a.verify_markers
        if lane in ('native','cpu') and cached and same_mode and timing.get('tiger_sha256') == tiger_hash and cached.get('returncode') == 0 and cached.get('input_sha256') == sha(graph) and (old / (lane + '.egg')).is_file() and cached.get('output_sha256') == sha(old / (lane + '.egg')):
            shutil.copyfile(old / (lane + '.egg'), target)
            records.append(dict(cached, reused_from=str(old)))
            (output / 'timing.json').write_text(json.dumps(dict(config, runs=records), indent=2) + '\n')
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
            result = subprocess.run([str(a.tiger), '--ilp-solver', 'cbc'], stdin=inp, stdout=out, stderr=err, env=env)
            seconds = time.perf_counter() - start
        records.append(dict(lane=lane, seconds=seconds, returncode=result.returncode,
                            input_sha256=sha(graph), output_sha256=sha(target)))
        (output / 'timing.json').write_text(json.dumps(dict(config, runs=records), indent=2) + '\n')
        result.check_returncode()
        pretty.unlink()
    checker = ROOT / 'target/analyzer/release/examples/tiger_output_check'
    command = [str(checker), str(output / 'quality.json')]+[str(output / (lane+'.egg')) for lane in lanes]
    if a.reference_output:
        command.append(str(a.reference_output))
    primary = subprocess.run(command)
    # Keep the original fused-native gate, but always record the independent
    # same-commit CPU/GPU gate too; a known fused difference must not hide it.
    pair = subprocess.run([str(checker), str(output / 'cpu-gpu-quality.json'),
                           str(output / 'cpu.egg'), str(output / 'gpu.egg')]) if 'gpu' in lanes else None
    primary.check_returncode()
    if pair:pair.check_returncode()
    print(json.dumps(records, indent=2))
    print(output / 'quality.json')

if __name__ == '__main__':
    main()
