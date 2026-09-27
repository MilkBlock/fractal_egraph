#!/usr/bin/env python3
"""Independent same-binary mini3 processes; compilation/Bake/verification separated."""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import re
import signal
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'experiments/math_mini3/source.egg'
BIN = ROOT / 'target/release/egg_layout'
RUNNER = ROOT / 'target/release/examples/virtual_math_bench'


def measure(command, prefix, timeout=30):
    env = dict(os.environ, EGG_LAYOUT_GRAPHS='0', EGG_LAYOUT_EAGER_SVG='0')
    for name in ('EGG_LAYOUT_FULL_TRACE', 'EGG_LAYOUT_RIPEN_HISTORY', 'EGG_LAYOUT_PROFILE'):
        env.pop(name, None)
    stdout, stderr = prefix.with_suffix('.stdout'), prefix.with_suffix('.stderr')
    begin = time.monotonic()
    with stdout.open('w') as out, stderr.open('w') as err:
        proc = subprocess.Popen(['/usr/bin/time', '-l', *map(str, command)], cwd=ROOT,
                                env=env, stdout=out, stderr=err, start_new_session=True)
        while True:
            try:
                code = proc.wait(timeout=.1)
                break
            except subprocess.TimeoutExpired:
                ps = subprocess.check_output(['ps', '-axo', 'pgid=,rss='], text=True)
                rss = sum(int(parts[1])*1024 for line in ps.splitlines()
                          if len(parts := line.split()) == 2 and parts[0] == str(proc.pid))
                if time.monotonic()-begin >= timeout or rss >= 4*1024**3:
                    os.killpg(proc.pid, signal.SIGKILL)
                    proc.wait()
                    raise RuntimeError(f'external benchmark guard (30s/4GiB): {command}')
    text = stderr.read_text()
    peak = re.search(r'(\d+)\s+maximum resident set size', text)
    wall = re.search(r'([\d.]+)\s+real\s+', text)
    if code or not peak or not wall:
        raise RuntimeError(f'failed measurement: {command}\n{text[-2000:]}')
    lines = stdout.read_text().splitlines()
    payload = json.loads(lines[-1]) if lines and lines[-1].startswith('{') else None
    return dict(command=list(map(str, command)), process_wall_seconds=float(wall[1]),
                peak_rss_bytes=int(peak[1]), observed_wall_seconds=time.monotonic()-begin,
                result=payload)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', required=True)
    p.add_argument('--runs', type=int, default=3)
    p.add_argument('--library', help='Reuse a supplied library instead of preparing a round-1 Bake')
    p.add_argument('--skip-analysis', action='store_true')
    a = p.parse_args()
    if a.runs < 1:
        p.error('--runs must be positive')
    out = (ROOT / a.output).resolve()
    out.mkdir(parents=True, exist_ok=False)
    report = dict(schema='math-virtual-bench/v1', platform=platform.platform(),
        environment={k:os.environ[k] for k in ('RAYON_NUM_THREADS','EGG_LAYOUT_ANALYSIS_THREADS','EGG_LAYOUT_QUERY_THREADS','EGG_LAYOUT_RIPEN_OBSERVE_MS','EGG_LAYOUT_RIPEN_OBSERVE_FACTS','EGG_LAYOUT_RIPEN_SWEEP_STEPS') if k in os.environ},
        commit=subprocess.check_output(
        ['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        working_tree_dirty=bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)),
        source=str(SOURCE.relative_to(ROOT)), source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
        runtime_binary_sha256=hashlib.sha256(RUNNER.read_bytes()).hexdigest(),
        external_guard={'seconds':30,'process_group_rss_bytes':4*1024**3},
        notes=['same mini3 source and three rounds', 'virtual intervention after round 1',
               'internal time includes source parsing, library loading, matching and packing',
               'worker time additionally includes report construction and engine/library drop; final stdout serialization excluded',
               'native means the same current kernel with no trace/Bake/virtualization; not the pristine egglog-baseline tag',
               'process RSS includes runtime/library/report allocations, excludes offline Bake and verification',
               'macOS time -l reports RSS in bytes; process wall clock has coarse resolution',
               'analysis workload includes trace/ripen and is not a native speedup comparison'], runs=[])
    def save():
        (out / 'measurements.json').write_text(json.dumps(report, indent=2) + '\n')
    save()
    if a.library:
        library = (ROOT / a.library).resolve()
    else:
        seed = out / 'bake-run'
        report['offline_capture'] = measure([BIN, 'analyze', '--source', SOURCE, '--rounds', '1',
                                            '--output', seed], out / 'offline-capture')
        library = out / 'library.json'
        report['offline_bake'] = measure([BIN, 'reuse-bake', library, seed], out / 'offline-bake')
        save()
    report['library_sha256'] = hashlib.sha256(library.read_bytes()).hexdigest()
    report['library_bytes'] = library.stat().st_size
    report['verification'] = measure([RUNNER, 'verify', SOURCE, library], out / 'verification')
    save()
    modes = ['native', 'match', 'virtual']
    for i in range(a.runs):
        for mode in modes[i % 3:] + modes[:i % 3]:
            r = measure([RUNNER, mode, SOURCE, library], out / f'{mode}-{i}')
            r.update(mode=mode, repetition=i)
            report['runs'].append(r)
            save()
            print(mode, r['result']['internal_seconds'], r['peak_rss_bytes'], flush=True)
    report['medians'] = {}
    for mode in modes:
        rs = [r for r in report['runs'] if r['mode'] == mode]
        report['medians'][mode] = dict(
            worker_seconds_including_drop=statistics.median(r['result']['worker_seconds_including_drop'] for r in rs),
            internal_seconds=statistics.median(r['result']['internal_seconds'] for r in rs),
            process_wall_seconds=statistics.median(r['process_wall_seconds'] for r in rs),
            peak_rss_bytes=statistics.median(r['peak_rss_bytes'] for r in rs),
            library_load_seconds=statistics.median(r['result']['library_load_seconds'] for r in rs),
            match_and_pack_seconds=statistics.median(r['result']['match_and_pack_seconds'] for r in rs))
    if not a.skip_analysis:
        report['analysis'] = measure([BIN, 'analyze', '--source', SOURCE, '--output', out / 'analysis'],
                                     out / 'analysis-run')
    save()
    print(json.dumps(report['medians'], indent=2))


if __name__ == '__main__':
    main()
