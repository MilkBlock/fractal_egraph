#!/usr/bin/env python3
"""Compare shared GPU with an exact, same-commit CPU lane; retain original gate."""
import argparse,csv,hashlib,json
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('report',type=Path);p.add_argument('--checked',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
r=json.loads(a.report.read_text());c=json.loads(a.checked.read_text())
for x in [r,c]:
 assert x['benchmark_finished'] and x['same_commit_verification']['exact_rows_values_equivalence_visibility']
 assert x['gpu']['runtime']['workers']==1 and x['gpu']['runtime']['shader_compiles']==1
 assert x['gpu']['device']['retained_matches']==0
assert not r['gpu']['audit'] and c['gpu']['audit']
assert r['source_hashes']==c['source_hashes'] and r['worker_sha256']==c['worker_sha256']
assert r['gpu_graph']==c['gpu_graph']
checks=sum(q['stats']['audit_queries'] for q in c['gpu']['rules'])
assert checks>len(c['gpu']['rules'])
g=r['gpu']['runtime'];cpu=r['sorted_commit_cpu']['runtime'];n=r['native'];device=r['gpu']['device']
old=json.loads((Path(__file__).resolve().parents[1]/'live-table-bench/results.json').read_text())
r['checked_replay']=dict(raw=str(a.checked.resolve()),sha256=hashlib.sha256(a.checked.read_bytes()).hexdigest(),audit_queries=checks,
    same_commit_verification=c['same_commit_verification'],original_graph_equal=c['complete'])
r['raw_report']=str(a.report.resolve());r['raw_sha256']=hashlib.sha256(a.report.read_bytes()).hexdigest()
r['speedup_same_commit_run']=cpu['run_seconds']/g['run_seconds']
r['speedup_same_commit_setup_run']=(cpu['setup_seconds']+cpu['run_seconds'])/(g['setup_seconds']+g['run_seconds'])
r['historical_per_rule']=dict(commit='7d6d9fc',gpu_run_seconds=old['gpu']['runtime']['run_seconds'],device_buffer_peak_sum=old['totals']['peak_device_bytes'],rpc_batches=old['totals']['dispatches'],uploads=old['totals']['uploaded_bytes'],note='Previous implementation and separate run; not a single-change ablation.')
a.output.mkdir(parents=True,exist_ok=True);(a.output/'results.json').write_text(json.dumps(r,indent=2)+'\n')
with (a.output/'rules.csv').open('w') as f:
 fields=['ruleset','rule',*r['gpu']['rules'][0]['stats']];w=csv.DictWriter(f,fields,lineterminator='\n');w.writeheader();w.writerows(dict(ruleset=x['ruleset'],rule=x['rule'],**x['stats']) for x in r['gpu']['rules'])
lines=['# Shared table runtime: real raytrace result','',
'One release run per lane on Apple M3 Max, one native thread. Actual saved production Pass-3 packet; original rules and schedule preserved. Not a complete compiler/extraction run. OS and Metal caches are not flushed; no averages.', '',
'| Lane | Setup s | Run s | Setup + run s |','|---|---:|---:|---:|',
f'| Native original fused search/apply | prefix already loaded | {n["run_seconds"]:.6f} | {n["run_seconds"]:.6f} |',
f'| CPU native queries, sorted commit | {cpu["setup_seconds"]:.6f} | {cpu["run_seconds"]:.6f} | {cpu["setup_seconds"]+cpu["run_seconds"]:.6f} |',
f'| GPU shared queries, same sorted commit | {g["setup_seconds"]:.6f} | {g["run_seconds"]:.6f} | {g["setup_seconds"]+g["run_seconds"]:.6f} |','',
f'The **same-commit** CPU/GPU comparison passes exact whole-graph equality: **{r["speedup_same_commit_run"]:.2f}×** run speedup, **{r["speedup_same_commit_setup_run"]:.2f}×** including preparation. The CPU lane executes cached native semi-naive queries; it is not a brute-force simulator. Its callback collection and sorted commit are measured. The original fused native lane is retained separately, not relabeled as the same-commit control or pristine.', '',
'| GPU execution component | Seconds |','|---|---:|',f'| Native event ingest / shared store | {g["event_seconds"]:.6f} |',f'| Packet encoding | {g["prepare_seconds"]:.6f} |',f'| GPU round trips | {g["roundtrip_seconds"]:.6f} |',f'| Device work, subset of round trips | {g["device_seconds"]:.6f} |',f'| Decode/sort | {g["decode_seconds"]:.6f} |',f'| Original RHS + merge/rebuild | {g["native_rhs_seconds"]:.6f} |','',
'Native original search+apply / insert-merge / rebuild: '+ ' / '.join(f'{n[k]:.6f} s' for k in ['search_apply_seconds','insert_merge_seconds','rebuild_seconds'])+'. Search+apply includes RHS staging. Device time must not be added to roundtrip time.', '',
'## Sharing and reclamation','', '| Measure | Result |','|---|---:|',f'| Workers / program installations | {g["workers"]} / {g["shader_compiles"]} |',f'| Shared native tables | {r["gpu"]["shared_tables"]} |',f'| Batch RPCs | {g["dispatches"]:,} |',f'| Live rows / allocated row slots | {r["gpu"]["live_rows"]:,} / {r["gpu"]["row_slots"]:,} |',f'| Reused row slots | {g["reused_slots"]:,} |',f'| Index-log compactions | {device["compactions"]:,} |',f'| Retained historical matches | {device["retained_matches"]} |',f'| Explicit device buffers | {device["device_bytes"]/2**20:.2f} MiB |',f'| Bulk upload / download | {g["uploaded_bytes"]:,} / {g["downloaded_bytes"]:,} bytes |','',
f'Historical per-rule implementation: {old["gpu"]["runtime"]["run_seconds"]:.3f} s run, {old["totals"]["dispatches"]:,} batch RPCs, {old["totals"]["peak_device_bytes"]/2**20:.2f} MiB summed worker buffer peaks. These are separately measured historical results, not an isolated one-change speedup. Device buffers are not process RSS and the original native egraph remains explicit.', '',
'## Correctness','', '| Check | Result |','|---|---|',f'| GPU vs CPU with identical commit policy | exact: {r["same_commit_verification"]["rows"]:,} public rows; facts, primitive values, visibility and equality |',f'| Independent native frontier checks, separate run | {checks:,} passed |',f'| Native handback unchanged | {r["native_continuation_unchanged"]} |',f'| Original fused engine exact graph | {r["complete"]} |',f'| Original fused engine cost comparison | {r["quality"]["same_extracted_costs"]}, {r["quality"]["native_expr_keys"]:,} Expr keys |','',
'Original-path difference: '+str(r['verification_error']), '',
'The original-path gate remains visible. Different ordering in the original fused path can choose different terms through custom merge, so its timing is not claimed as a strict same-graph speedup. The same-commit CPU reference reproduces the GPU graph entirely; no Smaller rows or selected fields are excluded from that primary comparison.', '',
'| Sort | Original native nodes/classes | Shared GPU nodes/classes |','|---|---:|---:|']
for s in ['Expr','Term','TermAndCost']:
 x=r['native_graph']['sorts'][s];y=r['gpu_graph']['sorts'][s];lines.append(f'| {s} | {x["enodes"]:,} / {x["eclasses"]:,} | {y["enodes"]:,} / {y["eclasses"]:,} |')
lines+=['','No baked Cell shortcut coverage is claimed: all original LHS atoms are represented. The checked run is excluded from timing. High-water buffer capacity remains allocated, but historical matching records are eliminated and event history is compacted without changing eligibility.', '',f'Raw reports: `{a.report.resolve()}` and `{a.checked.resolve()}`. Source, input, executable and worker hashes are retained in `results.json`.']
(a.output/'RESULTS.md').write_text('\n'.join(lines)+'\n')
