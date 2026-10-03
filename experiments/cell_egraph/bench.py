#!/usr/bin/env python3
"""One run per variant of the real analyze pipeline, not a cell-only microbench."""
import argparse, json, os, pathlib, subprocess, time
p = argparse.ArgumentParser()
p.add_argument('--rounds', type=int, default=3)
p.add_argument('--output', required=True)
a = p.parse_args()
root = pathlib.Path(__file__).resolve().parents[2]
out = root / a.output
out.mkdir(parents=True, exist_ok=False)
results = []
for label, enabled in [('raw', '0'), ('normalized', '1')]:
    cmd = [str(root/'target/release/egg_layout'), 'analyze', '--source', str(root/'egglog/tests/math-microbenchmark.egg'), '--rounds', str(a.rounds), '--output', str(out/label)]
    env = dict(os.environ, EGG_LAYOUT_GRAPHS='0', EGG_LAYOUT_CELL_BINDING_NORMALIZATION=enabled)
    started = time.perf_counter()
    with (out/(label+'.log')).open('w') as log:
        subprocess.run(cmd, env=env, cwd=root, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
    cells = json.loads((out/label/'cell-egraph.json').read_text())
    analysis = json.loads((out/label/'analysis.json').read_text())
    results.append(dict(variant=label, seconds=time.perf_counter()-started, rounds=a.rounds, summary=analysis['summary'], ripen_counts=analysis['saturated_rule_composition']['counts'],
                        cells={k:v for k,v in cells.items() if k not in ('nodes','units')}, command=cmd))
(out/'results.json').write_text(json.dumps(results, indent=2)+'\n')
print(json.dumps(results, indent=2))
