#!/usr/bin/env python3
"""Generate source-level views, then validate and time one selected view."""
import argparse
import hashlib
import json
import pathlib
import shutil
import statistics
import subprocess

p = argparse.ArgumentParser()
p.add_argument('--source', type=pathlib.Path, required=True)
p.add_argument('--output', type=pathlib.Path, required=True)
p.add_argument('--view', choices=['base', 'tuple'], required=True)
p.add_argument('--repeats', type=int, default=7)
p.add_argument('--binary', type=pathlib.Path, default=pathlib.Path('target/release/examples/eggcc_marker'))
p.add_argument('--motifs', type=pathlib.Path, default=pathlib.Path('experiments/eggcc_marker/motifs.json'))
a = p.parse_args()
if a.repeats < 1:
    p.error('--repeats must be positive')
out = a.output.resolve()
out.mkdir(parents=True, exist_ok=False)
binary = str(a.binary.resolve())
with (out / 'generate.log').open('w') as log:
    subprocess.run([binary, str(a.source.resolve()), str(a.motifs.resolve()), str(out / 'candidates'), 'inventory'], stdout=log, stderr=log, check=True)
manifest = json.loads((out / 'candidates/manifest.json').read_text())
motif = dict(kind='Nested', outer='HasType', slot=1, inner={'base': 'Base', 'tuple': 'TupleT'}[a.view])
selected = next((c for c in manifest['candidates'] if c['motif'] == motif), None)
if selected is None:
    raise SystemExit('No eligible repeated occurrence of this view in the supplied schedule.')
baseline = out / 'candidates/baseline.egg'
candidate = out / 'candidates' / selected['file']
with (out / 'measure.log').open('w') as log:
    subprocess.run([binary, 'measure', str(out / 'measurement'), str(a.repeats), str(baseline), str(candidate)], stdout=log, stderr=log, check=True)
m = json.loads((out / 'measurement/measurement.json').read_text())
accepted = all(m['profile_same_original_snapshot']) and all(s['same_original_snapshot'] for s in m['samples'])
def metrics(i):
    samples = [s for s in m['samples'] if s['variant'] == i]
    q = m['profiles'][i]
    return dict(execution_ms_median=1000 * statistics.median(s['execution_seconds'] for s in samples),
                total_ms_median=1000 * statistics.median(s['total_seconds'] for s in samples),
                scan_rows=q['scan_rows'], refine_rows=q['refine_rows'], key_lookups=q['key_lookups'], marker_rows=m['marker_rows'][i])
b, c = metrics(0), metrics(1)
paired = []
for r in range(a.repeats):
    samples = {s['variant']: s for s in m['samples'] if s['round'] == r}
    paired.append(100 * (1 - samples[1]['execution_seconds'] / samples[0]['execution_seconds']))
result = dict(scope=m['scope'], source_sha256=hashlib.sha256(a.source.read_bytes()).hexdigest(),
              binary_sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(), selected=selected,
              repeats=a.repeats, same_original_state_every_run=accepted,
              baseline=b, candidate=c,
              row_work_reduction_percent=100 * (1 - (c['scan_rows'] + c['refine_rows']) / (b['scan_rows'] + b['refine_rows'])),
              paired_execution_improvement_percent_median=statistics.median(paired),
              paired_execution_improvement_percent=paired,
              diagnostic_only_equivalent_except_to_hoist=m['diagnostic_only_equivalent_except_to_hoist'],
              timing_scope=m['timing'])
# A failed candidate is retained for diagnosis, but never published as optimized.
if accepted:
    shutil.copyfile(candidate, out / 'validated.egg')
    result['validated_program'] = 'validated.egg'
else:
    result['validated_program'] = None
    result['rejection'] = 'Full original-state equality failed. Diagnostic cache exclusion does not authorize adoption.'
(out / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({k: result[k] for k in ['same_original_state_every_run', 'baseline', 'candidate', 'row_work_reduction_percent', 'paired_execution_improvement_percent_median']}, indent=2))
