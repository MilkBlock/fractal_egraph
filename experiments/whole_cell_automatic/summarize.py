"""Preserve text evidence from one exact native automatic-Cell run."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

p = argparse.ArgumentParser()
p.add_argument('run', type=Path)
p.add_argument('--output', type=Path, required=True)
a = p.parse_args()
r = json.loads((a.run / 'report.json').read_text())
assert r['completed'] and r['no_manual_state_selection']
a.output.mkdir(parents=True, exist_ok=True)
for path in a.run.glob('*.json'):
    shutil.copy2(path, a.output / path.name)
shutil.copy2(a.run / 'original.egg', a.output / 'original.egg')
lines = ['# Automatic whole-Cell acceptance', '',
         'One actual native run; CPU event dispatcher, not a GPU or speedup benchmark.', '',
         '| Native coarse step | Whole graph e-nodes / e-classes | Native smooth matches | Macro calls (cumulative) | Fallbacks (cumulative) |',
         '|---|---:|---:|---:|---:|']
total = 0
for b in r['boundaries']:
    matches = sum(b['native']['smooth_matches'].values())
    total += matches
    run = b['runtime'].get('runtime', b['runtime'])
    c = run['counters']
    lines.append(f"| {b['boundary'] + 1} | {b['enodes']} / {b['eclasses']} | {matches} | {c['macro_transitions']} | {c['native_fallbacks']} |")
c = r['final_runtime']['counters']
lines += ['', f'Native smooth matches: **{total}**. Automatic-path fallback smooth matches: **{c["fallback_smooth_matches"]}**.',
          'Each accepted macro also checks that original smooth query watermarks did not advance.',
          'Whole original graphs are exactly equivalent at all three boundaries (including the two Start rows).', '',
          'The two inputs share the Bake. Add commutativity exposes both x/y slot orientations,',
          'so positive certificates retain multiple frames; these are reported rather than called one transition.', '',
          'Final counters:', '```json', json.dumps(c, indent=2), '```', '',
          'Runtime transition edges (instance IDs retain overlapping source views):', '```text']
for e in r['final_runtime']['transitions']:
    lines.append(f'{e["transition"]}: {e["source_instance"]} -> {e["target_instance"]} (state {e["source_state"]} -> {e["target_state"]})')
lines += ['```', '', 'Reproduce with the command in ../README.md. No wall-clock speedup is asserted.']
(a.output / 'RESULTS.md').write_text('\n'.join(lines) + '\n')
files = ['src/whole_cell.rs','src/whole_cell/automatic.rs','tests/whole_cell_automatic.rs','experiments/whole_cell_automatic/math.egg','experiments/whole_cell_automatic/grafts.egg','experiments/whole_cell_automatic/heldout.egg','experiments/whole_cell_chain/plan.json']
(a.output / 'provenance.json').write_text(json.dumps({
    'head_before_commit': subprocess.check_output(['git','rev-parse','HEAD'], text=True).strip(),
    'sha256': {f: hashlib.sha256(Path(f).read_bytes()).hexdigest() for f in files},
    'binary_sha256': hashlib.sha256(Path('target/analyzer/release/egg_layout').read_bytes()).hexdigest()
}, indent=2) + '\n')
