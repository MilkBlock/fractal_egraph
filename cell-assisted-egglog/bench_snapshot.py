#!/usr/bin/env python3
"""Benchmark a real eggcc schedule checkpoint, without tiny synthetic inputs.
Uses the existing native rule_repl scheduler. One timed execution, one diagnostic
replay, same frozen state including semi-naive timestamps. No bake or trace dump.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
CAPTURE = Path('/Users/mineralsteins/Repos/egg_related/egg_layout/out')
sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--snapshot',type=Path,default=CAPTURE/'raytrace-interactive-20261010/pass-3-flat.egg')
    p.add_argument('--capture-metadata',type=Path,default=CAPTURE/'eggcc-raytrace-pass-audit/measurement.json')
    p.add_argument('--breakpoint',default='/3/repeat0/4')
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--skip-build',action='store_true')
    a=p.parse_args();source=a.snapshot.resolve();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if not source.is_file():raise SystemExit('Missing production pass capture: use the eggcc-pass-benchmark skill to capture/flatten a real pass first.')
    binary=ROOT/'target/analyzer/release/examples/rule_repl'
    if not a.skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','rule_repl','--features','native-read-profile','--target-dir','target/analyzer'],cwd=ROOT,check=True)
    commands=f':until {a.breakpoint}\n:bench measured\n:quit\n'
    (out/'commands.txt').write_text(commands)
    provenance=dict(snapshot=str(source),snapshot_sha256=sha(source),binary_sha256=sha(binary),
        source_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        source_diff=subprocess.check_output(['git','diff','--stat'],cwd=ROOT,text=True),
        breakpoint=a.breakpoint,scope='Replay captured production pass initialization and original schedule prefix. Not a new production frontend/extraction run.')
    provenance['harness_sha256']={name:sha(ROOT/name) for name in ['examples/rule_repl.rs','examples/support/ruleset_bench.rs','cell-assisted-egglog/bench_snapshot.py']}
    if a.capture_metadata.is_file():
        provenance['capture_metadata']=json.loads(a.capture_metadata.read_text())
        workload=Path(provenance['capture_metadata']['source'])
        provenance['current_workload_sha256']=sha(workload) if workload.exists() else None
        assert provenance['current_workload_sha256']==provenance['capture_metadata']['source_sha256'], 'workload no longer matches saved capture'
    with (out/'replay.log').open('w') as log:
        subprocess.run([str(binary),str(source),str(out)],input=commands,text=True,cwd=ROOT,
            env=dict(os.environ,RAYON_NUM_THREADS='1',EGGLOG_PROBE_COUNTS_OFF='1'),stdout=log,stderr=log,check=True)
    r=json.loads((out/'measured.json').read_text())
    assert a.breakpoint in r['label'], 'requested checkpoint was skipped by native Repeat early termination'
    r['provenance']=provenance
    (out/'report.json').write_text(json.dumps(r,indent=2)+'\n')
    print(json.dumps({k:r[k] for k in ['label','wall_seconds','native','checks']},indent=2))
    print(out/'report.json')

if __name__=='__main__':main()
