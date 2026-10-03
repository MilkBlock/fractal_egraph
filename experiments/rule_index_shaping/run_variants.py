#!/usr/bin/env python3
"""Compare the engine's materialized join-index repertoire across rule variants.

Runs the same egglog program three times (base / rot / row, produced by
`rewrite.py`) and, for each run, reports:

  * which column indexes and composite tuple indexes the engine actually built,
    per table (`EGraph::materialized_indexes`, a read-only observer),
  * the rule report (search/apply, merge, rebuild, matches).

The point is to test whether *rule shape alone* can move the engine onto
different indexes, i.e. whether "indexing" can be done declaratively.
"""
import argparse
import json
import pathlib
import re
import subprocess
import sys

ap = argparse.ArgumentParser()
ap.add_argument('--dir', type=pathlib.Path, required=True)
ap.add_argument('--analyzer', type=pathlib.Path,
                default=pathlib.Path('target/release/egg_layout'))
a = ap.parse_args()
d = a.dir.resolve()

RUST = pathlib.Path('/Users/mineralsteins/Repos/egg_related/egg_layout')

# A tiny Rust driver is needed because the index observer is a library call.
# Instead of compiling one per run we use the existing debug binary hook when
# available; otherwise we fall back to the CLI's analysis report.
out = {}
for variant in ('base', 'rot', 'row'):
    src = d / f'{variant}.egg'
    run = d / f'run-{variant}'
    if run.exists():
        subprocess.run(['rm', '-rf', str(run)], check=True)
    env = dict(**__import__('os').environ, EGG_LAYOUT_GRAPHS='0',
               EGG_LAYOUT_INDEX_REPORT='1')
    env.pop('EGG_LAYOUT_COMPOSITION_DISCOVERY', None)
    cmd = [str((RUST / a.analyzer).resolve()), 'analyze', '--source', str(src),
           '--output', str(run)]
    tick = __import__('time').perf_counter()
    with (d / f'{variant}.log').open('wb') as log:
        proc = subprocess.run(cmd, env=env, stdout=log, stderr=log)
    secs = __import__('time').perf_counter() - tick
    out[variant] = dict(seconds=secs, exit=proc.returncode)
    print(f'{variant}: exit={proc.returncode} wall={secs:.2f}s', flush=True)

(d / 'runs.json').write_text(json.dumps(out, indent=2) + '\n')
print(json.dumps(out, indent=2))
