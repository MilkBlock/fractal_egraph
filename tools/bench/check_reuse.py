#!/usr/bin/env python3
"""Compare same-machine cargo bench results; never silently update the baseline."""
import argparse
import json


def compare(base, current, threshold):
    for key in ('schema', 'arch', 'os', 'scope'):
        if base.get(key) != current.get(key) or key not in base:
            raise ValueError(f'incompatible metadata: {key}')
    if base['schema'] != 'reuse-bench/v1':
        raise ValueError('unsupported schema')
    if not base['benchmarks'] or base['benchmarks'].keys() != current['benchmarks'].keys():
        raise ValueError('benchmark set changed; review and establish a new baseline')
    failures = []
    for name, old in base['benchmarks'].items():
        new = current['benchmarks'][name]
        a, b = old['median_ns'], new['median_ns']
        if not (0 < a < float('inf') and 0 < b < float('inf')) or old['batch'] != new['batch']:
            raise ValueError(f'invalid measurement: {name}')
        ratio = b / a - 1
        print(f'{name}: {a:.1f} -> {b:.1f} ns/op ({ratio:+.1%})')
        if ratio > threshold:
            failures.append(name)
    return failures


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('baseline')
    p.add_argument('current')
    p.add_argument('--threshold', type=float, default=.20)
    args = p.parse_args()
    if not 0 <= args.threshold < float('inf'):
        p.error('threshold must be finite and nonnegative')
    try:
        with open(args.baseline) as f:
            baseline = json.load(f)
        with open(args.current) as f:
            current = json.load(f)
        failures = compare(baseline, current, args.threshold)
        if failures:
            p.exit(1, f'Regression: {", ".join(failures)}\nRepeat on an idle machine before attributing the regression to code.\n')
    except (ValueError, KeyError, TypeError, OSError) as e:
        p.exit(2, f'Invalid comparison: {e}\n')
