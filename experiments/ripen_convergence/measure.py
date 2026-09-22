"""Small native ripen ablation. Build the release binary before running."""
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
    modes = {False: [], True: []}
    detail = None
    for repeat in range(a.repeats):
        for enabled in ([False, True] if repeat % 2 == 0 else [True, False]):
            dest = a.output / f'{case}-{repeat}-{int(enabled)}'
            cmd = [str(root/'target/release/egg_layout'), 'ripen-probe', str(dest), '12']
            if not enabled:
                cmd.append('--baseline')
            cmd += [str(root/'experiments/ripen_convergence'/f'{name}.egg') for name in files]
            result = subprocess.run(cmd, cwd=root, capture_output=True, text=True)
            if result.returncode:
                raise RuntimeError(result.stderr)
            data = json.loads((dest/'convergence.json').read_text())
            assert all(c['checks'] == 'passed' for c in data['cells'])
            modes[enabled].append(sum(c['seconds'] for c in data['cells']))
            if enabled:
                detail = data
    summary['cases'][case] = {
        'baseline_ripen_ms_median': statistics.median(modes[False])*1000,
        'observed_ripen_ms_median': statistics.median(modes[True])*1000,
        'index': detail['index'],
        'first_hit_rounds': [o['first_hit']['round'] for o in detail['opportunities']],
        'remaining_observed_rounds': [o['remaining_observed_rounds'] for o in detail['opportunities']],
        'actual_rounds_skipped': 0,
    }
(a.output/'summary.json').write_text(json.dumps(summary, indent=2)+'\n')
print(json.dumps(summary, indent=2))
