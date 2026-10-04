"""Run and summarize a full saved-history replay, preserving failure status."""
import argparse
import collections
import hashlib
import json
import os
import pathlib
import re
import subprocess
import time

p = argparse.ArgumentParser()
p.add_argument('--history', type=pathlib.Path, required=True)
p.add_argument('--output', type=pathlib.Path, required=True)
p.add_argument('--binary', type=pathlib.Path, default=pathlib.Path('target/release/egg_layout'))
p.add_argument('--thread-stack-bytes', type=int)
a = p.parse_args()
out = a.output.resolve()
out.mkdir(parents=True, exist_ok=False)
command = [str(a.binary.resolve()), 'analyze', '--replay-history', str(a.history.resolve()),
           '--output', str(out / 'run'), '--ripen-observe-ms', '50', '--ripen-observe-facts', '200000']
meta = dict(history_sha256=hashlib.sha256(a.history.read_bytes()).hexdigest(),
            binary_sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(),
            commit=subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
            working_diff_sha256=hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD'])).hexdigest(),
            command=command, environment={'EGG_LAYOUT_GRAPHS': '0', 'EGG_LAYOUT_ANALYSIS_THREADS': '2'},
            scope='Actual local native analyze replay of saved eggcc history; not pristine runtime or held-out coverage')
if a.thread_stack_bytes is not None:
    if a.thread_stack_bytes <= 0:
        raise SystemExit('thread stack must be positive')
    meta['environment']['RUST_MIN_STACK'] = str(a.thread_stack_bytes)
(out / 'invocation.json').write_text(json.dumps(meta, indent=2) + '\n')
start = time.monotonic()
with (out / 'run.log').open('w') as log:
    result = subprocess.run(['/usr/bin/time', '-l', *command], env={**os.environ, **meta['environment']},
                            stdout=log, stderr=log)
meta.update(exit_code=result.returncode, wall_seconds=time.monotonic() - start)
log = (out / 'run.log').read_text()
rss = re.search(r'(\d+)\s+maximum resident set size', log)
meta['maximum_resident_bytes_macos'] = int(rss[1]) if rss else None
footprint = re.search(r'(\d+)\s+peak memory footprint', log)
meta['peak_memory_footprint_bytes_macos'] = int(footprint[1]) if footprint else None
if 'stack overflow' in log:
    meta['failure'] = 'thread stack overflow'
analysis = out / 'run' / 'analysis.json'
meta['completed'] = result.returncode == 0 and analysis.exists()
if meta['completed']:
    report = json.loads(analysis.read_text())
    r = report['saturated_rule_composition']
    jobs = r['jobs']
    reasons = collections.Counter()
    observations = 0
    for job in jobs:
        if job.get('reason'):
            reason = job['reason'].split('\n')[0]
            reason = re.sub(r'^CS member \d+: ', '', reason)
            reason = re.sub(r': At .*', '', reason)
            reasons[reason] += 1
        if job.get('export', {}).get('observed_state', {}).get('file'):
            observations += 1
    meta['analysis'] = dict(jobs=len(jobs),
        prepared_and_registered=sum(isinstance(j.get('origin'), dict) for j in jobs),
        states=dict(collections.Counter(j['state'] for j in jobs)),
        exports=dict(collections.Counter(j.get('export', {}).get('status', 'none') for j in jobs)),
        completed_observation_exports=observations,
        rejected_reasons=dict(reasons), preparation=r.get('preparation_cache'),
        catalog=r.get('catalog'), limits=r.get('observation_limits'))
(out / 'summary.json').write_text(json.dumps(meta, indent=2) + '\n')
print(json.dumps({k: meta[k] for k in ('completed', 'exit_code', 'wall_seconds', 'maximum_resident_bytes_macos')}, indent=2))
raise SystemExit(result.returncode)
