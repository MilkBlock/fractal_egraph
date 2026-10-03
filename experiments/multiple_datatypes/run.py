#!/usr/bin/env python3
"""Offline real eggcc export -> native analyze -> history replay probe."""
import argparse
import collections
import hashlib
import json
import pathlib
import subprocess
import time

p = argparse.ArgumentParser()
p.add_argument('--eggcc', type=pathlib.Path, required=True)
p.add_argument('--analyzer', type=pathlib.Path, default=pathlib.Path('target/debug/egg_layout'))
p.add_argument('--output', type=pathlib.Path, required=True)
a = p.parse_args()
root = a.eggcc.resolve()
out = a.output.resolve()
out.mkdir(parents=True, exist_ok=True)
source = root / 'benchmarks/passing/bril/core/fact.bril'
export = [str(root / 'target/release/eggcc'), str(source), '--run-mode', 'egglog', '--stop-after-n-passes', '1']
with (out / 'source.egg').open('wb') as stdout, (out / 'export.log').open('wb') as stderr:
    subprocess.run(export, stdout=stdout, stderr=stderr, check=True)
import os
env = dict(os.environ, EGG_LAYOUT_GRAPHS='0')
env.pop('EGG_LAYOUT_COMPOSITION_DISCOVERY', None)
env.pop('EGG_LAYOUT_LAYER_VIEW', None)
def run(args, folder):
    tick = time.perf_counter()
    with (out / (folder + '.log')).open('wb') as log:
        subprocess.run([str(a.analyzer.resolve()), 'analyze', *args, '--output', str(out / folder)], env=env, stdout=log, stderr=log, check=True)
    elapsed = time.perf_counter() - tick
    r = json.loads((out / folder / 'analysis.json').read_text())
    s = r['saturated_rule_composition']
    return dict(summary=r['summary'], counts=s['counts'], rejected_reasons=dict(collections.Counter(j.get('reason', '') for j in s['jobs'] if j['state'] == 'Rejected')), elapsed_seconds=elapsed)
live = run(['--source', str(out / 'source.egg'), '--save-history'], 'live')
history = list((out / 'live').glob('*history*.json'))
assert len(history) == 1, history
replay = run(['--replay-history', str(history[0])], 'replay')
assert live['summary'] == replay['summary'], (live, replay)
assert live['counts'] == replay['counts'], (live, replay)
r = dict(scope='Actual local eggcc exported one optimizer pass; actual native egglog execution and trace analysis, not a synthetic prototype. Debug build; timings are not a speed benchmark.', export_command=export, eggcc_commit=subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip(), source_sha256=hashlib.sha256((out/'source.egg').read_bytes()).hexdigest(), source_lines=len((out/'source.egg').read_text().splitlines()), live=live, replay=replay, limitation='Trace ingestion is supported. Merge update replay still requires a pre-update table-state witness; rejected compositions do not demonstrate cell reuse.')
(out / 'report.json').write_text(json.dumps(r, indent=2) + '\n')
print(json.dumps(r, indent=2))
