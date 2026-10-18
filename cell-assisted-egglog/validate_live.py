#!/usr/bin/env python3
"""One native tiny-math run, one live run, exact whole-graph comparison.
This is runtime correctness evidence; debug/audited times are not a speedup bench.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from tools.bench.math_graph_equal import verify


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--analyzer', type=Path, default=ROOT/'target/analyzer/debug/egg_layout')
    ap.add_argument('--skip-build', action='store_true')
    args = ap.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    if not args.skip_build:
        subprocess.run(['cargo', 'build', '--offline', '--manifest-path', 'cell-assisted-egglog/Cargo.toml', '--target-dir', 'target/cell-assisted'], cwd=ROOT, check=True)
    worker = out/'matrix-metal'
    subprocess.run(['swiftc', '-O', ROOT/'cell-assisted-egglog/matrix.swift', '-o', worker], check=True)
    source = ROOT/'cell-assisted-egglog/matrix-live/math.egg'
    data = ROOT/'cell-assisted-egglog/matrix-live/math-input.egg'
    analysis = out/'analyze'
    import os
    with (out/'analyze.log').open('w') as log:
        subprocess.run([args.analyzer, 'analyze', '--source', source, '--output', analysis, '--save-history'], env=dict(os.environ, EGG_LAYOUT_GRAPHS='0'), cwd=ROOT, stdout=log, stderr=log, check=True)
    binary = ROOT/'target/cell-assisted/debug/cell-assisted-egglog'
    base = [binary, '--analysis', analysis, '--ruleset', '', '--consumer', 'factor', '--input', data]
    for mode, rounds in [('off', 8), ('matrix-live', 6)]:
        cmd = base + ['--mode', mode, '--rounds', str(rounds), '--dump', out/f'{mode}-graph.json']
        if mode == 'matrix-live':
            cmd += ['--native', 'add-commute,mul-commute', '--parts', '[{"atoms":[0],"cell":true},{"atoms":[1],"cell":true},{"atoms":[2],"cell":true}]', '--gpu-worker', worker, '--audit', 'true']
        with (out/f'{mode}.json').open('w') as f:
            subprocess.run(cmd, cwd=ROOT, stdout=f, check=True)
    eq = verify(out/'off-graph.json', out/'matrix-live-graph.json', eq_sorts=['Math'])
    live = json.loads((out/'matrix-live.json').read_text())['live']
    assert live['pending_matches'] == live['failed_matches'] == 0
    stats = live['stats']
    counts = stats['device']['execution_counts']
    assert all(c[1] <= 1 for c in counts), 'a completed binding was matched again'
    assert stats['bake_completed'] > 0
    assert all(n == 0 for n in stats['audit_uncovered'])
    device = stats.pop('device')
    report = dict(scope='actual fork native events, automatic native Bake, persistent MPS resume and original RHS; positive constructor scope only; not a speedup benchmark',
                  source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                  analyzer_sha256=hashlib.sha256(args.analyzer.read_bytes()).hexdigest(), worker_sha256=hashlib.sha256(worker.read_bytes()).hexdigest(),
                  graph_equality=eq, stats=stats,
                  matrix=dict(device=device['device'], channels=len(counts), logical_completions=sum(c[1] for c in counts), max_completions_per_channel=max(c[1] for c in counts), bytes=device['matrix_bytes']),
                  caveats=['CPU keyed source proof admission and fragment enumeration remain', 'Native oracle queries enabled only for this audit', 'Local Bake may be unfinished; never certified on timeout', 'Not whole eggcc coverage'])
    (out/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(out/'report.json')


if __name__ == '__main__':
    main()
