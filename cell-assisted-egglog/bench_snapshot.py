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
    p.add_argument('--pre-query-run',help='in conformance mode, execute this native schedule expression at the checkpoint before checking queries')
    p.add_argument('--assisted-worker',type=Path,help='validate all packet queries on GPU against native snapshot matches, instead of timing the packet')
    p.add_argument('--table-worker',type=Path,help='time native packet, then benchmark binary GPU table queries at its checkpoint')
    p.add_argument('--live-worker',type=Path,help='execute the complete original packet with persistent GPU queries and native RHS')
    p.add_argument('--live-check',action='store_true',help='diagnostic full native query oracle at every live boundary; not a timing run')
    a=p.parse_args();source=a.snapshot.resolve();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if not source.is_file():raise SystemExit('Missing production pass capture: use the eggcc-pass-benchmark skill to capture/flatten a real pass first.')
    binary=ROOT/'target/analyzer/release/examples/rule_repl'
    if not a.skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','rule_repl','--features','native-read-profile','--target-dir','target/analyzer'],cwd=ROOT,check=True)
    if sum(bool(x) for x in [a.assisted_worker,a.table_worker,a.live_worker])>1:raise SystemExit('choose one GPU backend')
    if a.live_check and not a.live_worker:raise SystemExit('--live-check requires --live-worker')
    if a.live_worker and a.pre_query_run:raise SystemExit('live benchmark starts at the original packet boundary')
    operation=f':check-assisted measured {a.assisted_worker.resolve()}' if a.assisted_worker else ':bench measured'
    if a.pre_query_run and not (a.assisted_worker or a.table_worker):raise SystemExit('--pre-query-run requires a query worker')
    advance=f'(run-schedule {a.pre_query_run})\n' if a.pre_query_run else ''
    if a.live_worker:
        commands=f':until {a.breakpoint}\n:bench-live measured {a.live_worker.resolve()}'+(' check' if a.live_check else '')+'\n:quit\n'
    elif a.table_worker:
        commands=f':until {a.breakpoint}\n:bench native-packet\n{advance}:bench-tables measured {a.table_worker.resolve()}\n:quit\n'
    else:commands=f':until {a.breakpoint}\n{advance}{operation}\n:quit\n'
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
    if a.assisted_worker or a.table_worker:
        final=json.loads((out/'final.json').read_text());plan=json.loads((out/'schedule.json').read_text())
        r['label']=plan[final['cursor']]['label']
        r['worker_sha256']=sha(a.assisted_worker or a.table_worker)
        r['pre_query_run']=a.pre_query_run
    assert a.breakpoint in r['label'], 'requested checkpoint was skipped by native Repeat early termination'
    if a.table_worker:
        r['native_packet']=json.loads((out/'native-packet.json').read_text())
        r['source_hashes']={str(p.relative_to(ROOT)):sha(p) for p in [*sorted((ROOT/'cell-assisted-egglog/src').glob('*.rs')),ROOT/'cell-assisted-egglog/matrix.swift',ROOT/'cell-assisted-egglog/tables.metal',ROOT/'cell-assisted-egglog/integer.metal',ROOT/'examples/support/cell_query_validation.rs']}
        n=r['native_packet']['native']
        assert n['search_and_apply_seconds']>n['insert_merge_seconds']+n['rebuild_seconds'], 'not search dominated'
        assert r['complete'] and all(x['exact_native_binding_set'] for x in r['rules']), 'query validation failed'
    if a.live_worker:
        r['worker_sha256']=sha(a.live_worker)
        r['source_hashes']={str(p.relative_to(ROOT)):sha(p) for p in [*sorted((ROOT/'cell-assisted-egglog/src').glob('*.rs')),ROOT/'cell-assisted-egglog/matrix.swift',ROOT/'cell-assisted-egglog/tables.metal',ROOT/'cell-assisted-egglog/family.metal',ROOT/'cell-assisted-egglog/family_cells.metal',ROOT/'cell-assisted-egglog/family_events.metal',ROOT/'cell-assisted-egglog/family_ports.metal',ROOT/'cell-assisted-egglog/integer.metal',ROOT/'egglog/src/cell_assist.rs',ROOT/'egglog/src/cell_events.rs',ROOT/'egglog/egglog-bridge/src/lib.rs',ROOT/'egglog/egglog-bridge/src/rule.rs',ROOT/'examples/support/live_table_bench.rs']}
        assert r['benchmark_finished'], 'live execution did not finish'
        r['strict_same_result_speedup_eligible']=bool(r['complete'])
        r['same_commit_speedup_eligible']=r.get('same_commit_verification') is not None
        assert r['same_commit_speedup_eligible'], r.get('same_commit_verification_error')
    r['provenance']=provenance
    (out/'report.json').write_text(json.dumps(r,indent=2)+'\n')
    keys=['label','complete','rule_count','matched_bindings'] if (a.assisted_worker or a.table_worker) else ['label','wall_seconds','native','checks']
    if a.live_worker:keys=['label','complete','native','verification']
    print(json.dumps({k:r[k] for k in keys},indent=2))
    print(out/'report.json')

if __name__=='__main__':main()
