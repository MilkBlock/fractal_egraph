"""Small native ripen ablation. Build the release binary before running."""
import os
import argparse
import json
import pathlib
import statistics
import subprocess

p = argparse.ArgumentParser()
p.add_argument('output', type=pathlib.Path)
p.add_argument('--repeats', type=int, default=5)
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=False)
root = pathlib.Path(__file__).resolve().parents[2]
summary = {'scope': 'native isolated ripen observer, no execution skipped', 'repeats': a.repeats, 'cases': {}}
for case, files in [('chain', ['a', 'b']), ('math_ac', ['math-right', 'math-left'])]:
    modes = {'baseline': [], 'snapshot': [], 'packets': []}
    details = {}
    for repeat in range(a.repeats):
        for mode in (['baseline','snapshot','packets'] if repeat % 2 == 0 else ['packets','snapshot','baseline']):
            dest = a.output / f'{case}-{repeat}-{mode}'
            cmd = [str(root/'target/release/egg_layout'), 'ripen-probe', str(dest), '12']
            if mode == 'baseline':
                cmd.append('--baseline')
            cmd += [str(root/'experiments/ripen_convergence'/f'{name}.egg') for name in files]
            env = dict(os.environ)
            env.pop('EGG_LAYOUT_RIPEN_PACKET_AUDIT', None)
            env.pop('EGG_LAYOUT_RIPEN_SNAPSHOT_PROBE', None)
            if mode == 'snapshot':
                env['EGG_LAYOUT_RIPEN_SNAPSHOT_PROBE'] = '1'
            result = subprocess.run(cmd, cwd=root, capture_output=True, text=True, env=env)
            if result.returncode:
                raise RuntimeError(result.stderr)
            data = json.loads((dest/'convergence.json').read_text())
            assert all(c['checks'] == 'passed' for c in data['cells'])
            modes[mode].append(sum(c['seconds'] for c in data['cells']))
            details[mode] = data
    detail = details['packets']
    summary['cases'][case] = {
        'ripen_ms_median': {mode: statistics.median(times)*1000 for mode,times in modes.items()},
        'index': detail['index'],
        'first_hit_rounds': [o['first_hit']['round'] for o in detail['opportunities']],
        'remaining_observed_rounds': [o['remaining_observed_rounds'] for o in detail['opportunities']],
        'snapshot_exports': {mode: sum(c['convergence']['snapshot_exports'] for c in d['cells']) for mode,d in details.items()},
        'packet_updates': sum(c['convergence']['packet_updates'] for c in detail['cells']),
        'fallback_reasons': [c['convergence']['fallback_reasons'] for c in detail['cells']],
        'actual_rounds_skipped': 0,
    }
(a.output/'summary.json').write_text(json.dumps(summary, indent=2)+'\n')
print(json.dumps(summary, indent=2))
