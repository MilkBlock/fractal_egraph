#!/usr/bin/env python3
"""Native planner scan evidence, including a losing single-consumer control.
Build first: cargo build --release --offline --features native-read-profile
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import subprocess
import tempfile


def workload(consumers, bindings):
    code = [
        '(datatype E (V i64) (A E) (B E))',
        '(rewrite (A x) (B x) :name "a-to-b")',
    ]
    for i in range(consumers):
        code.extend([
            f'(relation Flag{i} (E))',
            f'(relation Done{i} (E))',
            f'(rule ((= root (A x)) (= root (B x)) (Flag{i} x)) '
            f'((Done{i} root)) :name "consumer{i}")',
        ])
    for x in range(bindings):
        code.append(f'(A (V {x}))')
        code.extend(f'(Flag{i} (V {x}))' for i in range(consumers))
    code.append('(run 3)')
    return '\n'.join(code) + '\n'


def main():
    repo = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=repo / 'target/release/egg_layout')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--bindings', type=int, default=64)
    parser.add_argument('--consumers', type=int, nargs='+', default=[1, 8, 16])
    args = parser.parse_args()
    if args.output.exists():
        parser.error('output already exists; keep previous evidence')
    if args.bindings < 1 or any(n < 1 for n in args.consumers):
        parser.error('bindings and consumers must be positive')
    binary = args.binary.resolve()
    env = dict(os.environ, EGG_LAYOUT_GRAPHS='0', EGG_LAYOUT_RIPEN_OBSERVE_MS='60000')

    def run(*command):
        result = subprocess.run([str(binary), *map(str, command)], env=env, capture_output=True, text=True)
        if result.returncode:
            raise RuntimeError(result.stderr)
        return json.loads(result.stdout)

    results = []
    with tempfile.TemporaryDirectory(prefix='reuse-scans-') as tmp:
        root = Path(tmp)
        seed = root / 'seed'
        library = root / 'library.json'
        run('analyze', '--source', repo / 'experiments/reuse_feedback/seed.egg', '--output', seed)
        run('reuse-bake', library, seed)
        for consumers in args.consumers:
            source = root / f'fanout-{consumers}.egg'
            source.write_text(workload(consumers, args.bindings))
            report = run('reuse-run', source, library, '--read-profile')
            profile = report['read_profile']
            if not profile or not report['reference_equivalent']:
                raise RuntimeError('missing profile or failed native equivalence')
            optimized = profile['optimized']['total']['scan_rows']
            native = profile['reference']['total']['scan_rows']
            results.append({
                'consumers': consumers, 'bindings': args.bindings,
                'views': report['views'], 'reference_equivalent': report['reference_equivalent'],
                'feature_filter': report['feature_filter'], 'read_profile': profile,
                'scan_reduction_fraction': 1 - optimized / native if native else None,
            })
    evidence = {
        'measured_at': datetime.datetime.now().astimezone().isoformat(),
        'scope': 'actual native egglog search/apply table row visits, including view refresh and lazy indexes; no throughput or mutation-count claim',
        'instrumentation': 'native-read-profile Cargo feature; all default-build counter code compiled out',
        'workload': 'seeded A/B fragment; deterministic consumer fanout; three source rounds; independently verified all-user-table final state',
        'results': results,
    }
    with args.output.open('x') as stream:
        json.dump(evidence, stream, indent=2)
        stream.write('\n')
    for result in results:
        p = result['read_profile']
        print(f"consumers={result['consumers']}: scans {p['reference']['total']['scan_rows']} -> {p['optimized']['total']['scan_rows']}, reduction={result['scan_reduction_fraction']:.1%}")


if __name__ == '__main__':
    main()
