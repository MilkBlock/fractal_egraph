#!/usr/bin/env python3
"""Controlled codec ablation on one immutable native history, without tier0 runs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tempfile

FLAGS = ['EGG_LAYOUT_EAGER_DICTIONARY', 'EGG_LAYOUT_DISABLE_RECUT',
         'EGG_LAYOUT_DISABLE_ROTATIONS']
ARMS = {'eager': FLAGS, 'probation': FLAGS[1:],
        'incremental': FLAGS[2:], 'full': []}


def stats(value):
    if isinstance(value, dict):
        if 'cut_passes' in value:
            return value
        for child in value.values():
            found = stats(child)
            if found is not None:
                return found
    return None


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--history', required=True, type=Path)
    p.add_argument('--output', required=True, type=Path)
    p.add_argument('--binary', default='target/release/egg_layout', type=Path)
    p.add_argument('--repeats', type=int, default=3)
    a = p.parse_args()
    if a.repeats < 1:
        p.error('--repeats must be positive')
    a.output.mkdir(parents=True, exist_ok=False)
    history, binary = a.history.resolve(), a.binary.resolve()

    def run(arm, dest, timing=False):
        env = dict(os.environ)
        for flag in FLAGS:
            env.pop(flag, None)
        env.update({flag: '1' for flag in ARMS[arm]})
        cmd = [str(binary), 'analyze', '--replay-history', str(history),
               '--output', str(dest)]
        mac_time = timing and platform.system() == 'Darwin'
        if mac_time:
            cmd = ['/usr/bin/time', '-l', *cmd]
        result = subprocess.run(cmd, env=env, text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)
        summary = json.JSONDecoder().raw_decode(result.stdout[result.stdout.index('{'):])[0]
        metric = {'tier1_seconds': summary['timings_seconds']['tier1'],
                  'whole_replay_seconds': summary['wall_seconds']}
        if mac_time:
            metric['process_peak_rss_bytes'] = int(re.search(
                r'(\d+)\s+maximum resident set size', result.stderr).group(1))
        return stats(json.loads((dest / 'analysis.json').read_text())), metric

    report = {'scope': 'Identical certified native history replay; no tier0 execution. '
              'Model units are not bytes. Timings and RSS include other tiers/export.',
              'history_sha256': hashlib.sha256(history.read_bytes()).hexdigest(),
              'arms': {}, 'performance': {}}
    for arm in ARMS:
        report['arms'][arm], _ = run(arm, a.output / arm)
        print(arm, report['arms'][arm], flush=True)
    for _ in range(a.repeats):
        for arm in ['eager', 'full']:
            with tempfile.TemporaryDirectory(prefix='comb-replay-') as temp:
                measured_stats, metric = run(arm, Path(temp) / 'run', timing=True)
            if measured_stats != report['arms'][arm]:
                raise RuntimeError('Non-deterministic replay statistics: ' + arm)
            report['performance'].setdefault(arm, []).append(metric)
    (a.output / 'results.json').write_text(json.dumps(report, indent=2) + '\n')
    print(a.output / 'results.json')


if __name__ == '__main__':
    main()
