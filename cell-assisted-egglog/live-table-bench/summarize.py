#!/usr/bin/env python3
"""Summarize a continuous run, keeping strict graph failures visible."""
import argparse,json,csv,hashlib
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('report',type=Path);p.add_argument('--checked',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
r=json.loads(a.report.read_text());check=json.loads(a.checked.read_text())
assert r['benchmark_finished'] and check['benchmark_finished']
assert not r['gpu']['audit'] and check['gpu']['audit']
assert r['worker_sha256']==check['worker_sha256']
assert r['source_hashes']==check['source_hashes']
q=[x['stats'] for x in r['gpu']['rules']]
sums={k:sum(x[k] for x in q) for k in q[0]}
assert sums['shader_compiles']==len(q)
assert sums['seeded_old_rows']>0 and sums['dispatches']>len(q)
assert r['native']['search_apply_seconds']>r['native']['insert_merge_seconds']+r['native']['rebuild_seconds']
checks=sum(x['stats']['audit_queries'] for x in check['gpu']['rules'])
assert checks>len(q)
r['totals']=sums
r['checked_replay']={k:check[k] for k in ['complete','verification_error','quality','native_continuation_unchanged','projection_without_smaller','projection_error','native_graph','gpu_graph']}
r['checked_replay'].update(audit_queries=checks,raw_report=str(a.checked.resolve()),raw_sha256=hashlib.sha256(a.checked.read_bytes()).hexdigest())
r['raw_report']=str(a.report.resolve());r['raw_sha256']=hashlib.sha256(a.report.read_bytes()).hexdigest()
a.output.mkdir(parents=True,exist_ok=True)
(a.output/'results.json').write_text(json.dumps(r,indent=2)+'\n')
fields=['ruleset','rule',*q[0]]
with (a.output/'rules.csv').open('w') as f:
 w=csv.DictWriter(f,fields,lineterminator='\n');w.writeheader();w.writerows(dict(ruleset=x['ruleset'],rule=x['rule'],**x['stats']) for x in r['gpu']['rules'])
n=r['native'];g=r['gpu']['runtime']
lines=['# Continuous raytrace runtime result','',
'Actual production-derived Pass-3 packet, original 14-rule schedule. One fresh release execution per lane, Apple M3 Max, one native thread; no averages. Current native fork, not pristine. Offline prefix replay and clones are excluded from both lanes. No solver calls or per-round shader compilation.', '',
'| Full packet | Seconds |','|---|---:|',f'| Native run | {n["run_seconds"]:.6f} |',f'| Native search + apply | {n["search_apply_seconds"]:.6f} |',f'| Native insert/merge | {n["insert_merge_seconds"]:.6f} |',f'| Native rebuild | {n["rebuild_seconds"]:.6f} |',f'| Assisted one-time setup | {g["setup_seconds"]:.6f} |',f'| Assisted continuous run | {g["run_seconds"]:.6f} |',f'| Assisted setup + run | {g["setup_seconds"]+g["run_seconds"]:.6f} |','',
'## Assisted run breakdown','', '| Component | Seconds |','|---|---:|',
f'| CPU event routing | {g["event_seconds"]:.6f} |',f'| CPU fragment projection/encoding | {sums["prepare_seconds"]:.6f} |',f'| Binary GPU round trips | {sums["roundtrip_seconds"]:.6f} |',f'| GPU device work (subset of round trips) | {sums["device_seconds"]:.6f} |',f'| Binding decode/sort | {sums["decode_seconds"]:.6f} |',f'| Acknowledgements | {sums["acknowledge_seconds"]:.6f} |',f'| Original native RHS + merge/rebuild | {g["native_rhs_seconds"]:.6f} |','',
'Device time is not additive to roundtrip time. Native search+apply includes RHS staging. The assisted native action timer contains its private binding lookup and original RHS, not the original full relational Join. All host work remains in the assisted run total.', '',
'## Evidence that this is incremental','', '| Measure | Value |','|---|---:|',f'| Rules / worker installations | {len(q)} / {sums["shader_compiles"]} |',f'| Native / assisted ruleset runs | {n["iterations"]:,} / {g["ruleset_runs"]:,} |',f'| GPU delta dispatches | {sums["dispatches"]:,} |',f'| Quiet rule calls, no GPU query | {sums["quiet_runs"]:,} |',f'| Old bootstrap fragment rows indexed without matching | {sums["seeded_old_rows"]:,} |',f'| First uploads, bytes | {sums["first_upload_bytes"]:,} |',f'| Later uploads, bytes | {sums["uploaded_bytes"]-sums["first_upload_bytes"]:,} |',f'| Downloaded binding records, bytes | {sums["downloaded_bytes"]:,} |',f'| Native logical matches / assisted emitted bindings | {n["logical_matches"]:,} / {sums["emitted_bindings"]:,} |','',
'Old rows are retained across native events and compiled plans survive the entire packet. Only new accepted bindings are returned in the timed lane. Match counts are not committed mutation counts. Rule-level workers currently duplicate shared tables; this is not one globally shared GPU table store.', '',
'## Correctness gates','', '| Gate | Result |','|---|---|',f'| Strict final rows/values/equality/visibility | {r["complete"]} |',f'| Exact graph after explicitly excluding Smaller rows | {r["projection_without_smaller"] is not None} |',f'| Same ExtractedExpr cost sets | {r["quality"]["same_extracted_costs"]} ({r["quality"]["native_expr_keys"]:,} keys) |',f'| Native handback produces no further graph change | {r["native_continuation_unchanged"]} |',f'| Separate native per-boundary delta-query checks | {checks:,} passed |','',
'Full graph difference: '+str(r['verification_error']), '', 'Projection difference: '+str(r['projection_error']), '', 'Equal Expr/Term counts are not a graph-equivalence proof. Bootstrap fragment counts include repeated projections of the same native table across rule workers.', '',
'The Smaller-excluded check still verifies every other table, primitive value, visibility and equality modulo IDs; it is not a node-count approximation. It is reported as a separate projection, never a replacement for strict whole-graph equality. Equal costs do not by themselves prove identical selected syntax. No actual compiler extraction or frontend rerun occurs inside this isolated packet.', '',
'| Sort | Native nodes / classes | Assisted nodes / classes |','|---|---:|---:|']
for s in ['Expr','Term','TermAndCost']:
 l=r['native_graph']['sorts'][s];v=r['gpu_graph']['sorts'][s];lines.append(f'| {s} | {l["enodes"]:,} / {l["eclasses"]:,} | {v["enodes"]:,} / {v["eclasses"]:,} |')
lines += ['',f'Total table rows: {r["native_graph"]["table_rows"]:,} / {r["gpu_graph"]["table_rows"]:,}.', '',
'**No validated strict same-graph speedup is claimed.** The continuous run and the cold-query experiment are distinct. This runtime currently uses explicit table fragments; it does not exploit baked Cell guarantees to omit internal LHS atoms. Certified Cell coverage is unmeasured, not assumed complete.', '',
'The separate checked replay is excluded from all timings above. Its executable/source/worker provenance is checked; the timed and checked reports retain all result gates separately. Sum of per-worker allocated buffer peaks is recorded in JSON; it is not process RSS.', '',
'Raw outputs: `'+str(a.report.resolve().parent)+'` and `'+str(a.checked.resolve().parent)+'`. See README for reproduction and the native handoff contract.']
(a.output/'RESULTS.md').write_text('\n'.join(lines)+'\n')
