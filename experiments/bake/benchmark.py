#!/usr/bin/env python3
"""Optional experiment harness. Production Bake/use/eval remain single Rust processes."""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess

p = argparse.ArgumentParser()
p.add_argument('--output', type=Path, required=True)
p.add_argument('--repeats', type=int, default=3)
a = p.parse_args()
if a.repeats < 1:
    p.error('repeats must be positive')
root = Path(__file__).resolve().parents[2]
out = a.output.resolve()
out.mkdir(parents=True, exist_ok=False)
exe = root / 'target/release/egg_layout'
env = dict(os.environ)
env.pop('EGG_LAYOUT_BINDING_ALGEBRA', None)
env['EGG_LAYOUT_DISCOVER_RECURSION'] = '1'

def run(name, *args):
    with (out / f'{name}.log').open('wb') as log:
        subprocess.run([str(exe), *map(str, args)], cwd=root, env=env,
                       stdout=log, stderr=subprocess.STDOUT, check=True)

def read(path):
    return json.loads(path.read_text())

library_dir = out / 'library'
run('bake', 'bake', root / 'experiments/bake/manifest.json', library_dir)
library = library_dir / 'library.egg'
original = library.read_bytes()
rows = []
for i in range(a.repeats):
    frozen, full = out / f'frozen-{i}', out / f'discovery-{i}'
    run(f'frozen-{i}', 'bake-use', library,
        root / 'experiments/bake/heldout-binary.egg', frozen, '--save-history')
    run(f'discovery-{i}', 'analyze', '--replay-history', frozen / 'history.json', '--output', full)
    f = read(frozen / 'use.json')
    d = read(full / 'analysis.json')
    assert set(f['covered_events']) == {int(x) for x in d['recursive_patterns']['event_witnesses']}
    assert f['templates_added'] == 0 and f['discovery_calls'] == 0
    times = d['summary']['timings_seconds']
    analysis = sum(times[k] for k in ('tier1', 'tier2', 'fractal_views', 'recursive_patterns'))
    rows.append({'fixed_matching_seconds': f['fixed_match_seconds'],
                 'full_analysis_seconds': analysis,
                 'eligible_events': f['eligible_events'],
                 'covered_events': len(f['covered_events']),
                 'contract_instances': f['contract_instances'],
                 'early_pending_seen': any(b['pending_units'] > 0 for b in f['capture_batches'])})
assert original == library.read_bytes()
families = read(library_dir / 'bake.json')['families']
linear = next(t for t in families if t['kind'] == 'linear_extension')
branch = next(t for t in families if t['kind'] == 'recursive_dag')
run('counter-query', 'bake-eval', library, linear['id'], 31, 200, out / 'counter-query.json')
run('frontier-query', 'bake-eval', library, branch['id'], 11, '--depth', 20, out / 'frontier-query.json')
run('uncovered', 'bake-use', library, root / 'experiments/bake/uncovered.egg', out / 'uncovered')
miss = read(out / 'uncovered/use.json')
assert not miss['matches'] and miss['templates_added'] == 0
fixed = statistics.median(r['fixed_matching_seconds'] for r in rows)
full = statistics.median(r['full_analysis_seconds'] for r in rows)
summary = {
    'scope': 'Release native egglog. Fixed-library recognition versus full metadata analysis on identical captured events. The full path also builds analysis graphs and richer diagnostics; this is not a tier0 execution speedup.',
    'bake': {k: v for k, v in read(library_dir / 'bake.json').items() if k != 'families'},
    'runs': rows,
    'median_fixed_matching_seconds': fixed,
    'median_full_analysis_seconds': full,
    'analysis_stage_ratio': full / fixed,
    'counter_query': read(out / 'counter-query.json'),
    'frontier_query': read(out / 'frontier-query.json'),
    'uncovered_without_learning': len(miss['uncovered_events']),
}
(out / 'results.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary, indent=2))
