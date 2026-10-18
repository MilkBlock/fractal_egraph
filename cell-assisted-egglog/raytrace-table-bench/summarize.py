#!/usr/bin/env python3
"""Compact the real checkpoint report without calling query time packet time."""
import argparse,csv,json,hashlib
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('report',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
r=json.loads(a.report.read_text());n=r['native_packet']
assert r['complete'] and all(x['exact_native_binding_set'] for x in r['rules'])
assert n['checks']['exact_final_rows_equivalence_and_visibility']
assert n['native']['search_and_apply_seconds']>n['native']['insert_merge_seconds']+n['native']['rebuild_seconds']
a.output.mkdir(parents=True,exist_ok=True)
keys=['compile_seconds','worker_setup_seconds','table_prepare_seconds','roundtrip_seconds','decode_seconds','gpu_cold_query_seconds','native_query_including_compile_seconds','uploaded_bytes','downloaded_bytes']
totals={k:sum(x[k] for x in r['rules']) for k in keys}
totals['gpu_device_seconds']=sum(x['device']['gpu_seconds'] for x in r['rules'])
r['totals']=totals
# Keep original rules, table sizes and work counters, omit repeated eligibility
# rule text; the raw full report remains in the output directory.
r['native_packet'].pop('resident_support',None)
used={t['table'] for x in r['rules'] for t in x['tables']}
for boundary in ['before','after']:
    snap=n[boundary];tables=snap['tables']
    snap['tables']={k:v for k,v in tables.items() if k in used}
    snap['other_table_count_omitted']=len(tables)-len(snap['tables'])
n['diagnostic']['table_reads']=[x for x in n['diagnostic']['table_reads'] if x[0] in used]
r['raw_report_sha256']=hashlib.sha256(a.report.read_bytes()).hexdigest()
r['raw_report']=str(a.report.resolve())
(a.output/'results.json').write_text(json.dumps(r,indent=2)+'\n')
fields=['ruleset','rule','table_atoms','guards','matched_bindings',*keys]
with (a.output/'queries.csv').open('w') as f:
 w=csv.DictWriter(f,fields,lineterminator='\n');w.writeheader();w.writerows({k:x[k] for k in fields} for x in r['rules'])
lines=['# Real raytrace search benchmark','',
'One release measurement, 2026-10-18, Apple M3 Max, one native worker thread. No averages. Same current native fork, not pristine. Original input, native checkpoint and executable hashes are in `results.json`.',
'', '## Original 14-rule saturation packet','', '| Component | Seconds |', '|---|---:|', f'| Packet wall | {n["wall_seconds"]:.6f} |']
for key,label in [('search_and_apply_seconds','Search + apply (includes RHS staging)'),('insert_merge_seconds','Insert + merge commits'),('rebuild_seconds','Rebuild')]:lines.append(f'| {label} | {n["native"][key]:.6f} |')
lines += ['',f'{n["native"]["iterations"]:,} native ruleset iterations; {n["native"]["matches"]:,} logical matches; {n["before"]["table_rows"]:,} → {n["after"]["table_rows"]:,} table rows. Separate diagnostic replay has identical canonical rows, values, visibility and per-rule match counts. Parent checkpoint is unchanged. No extraction occurs inside this packet.', '',
'## Complete queries at one real state inside that packet','',
f'The GPU uses explicit original table rows and the binary indexed table backend. This isolates backend matching; it neither invents Cell certificates nor applies original RHS actions. Before querying, execute the original `(seq terms terms-helpers)` once to expose real enabled cost updates. All 14 query binding sets agree exactly with native, totaling {r["matched_bindings"]:,} bindings.', '',
'| Component, summed across 14 queries | Seconds |','|---|---:|',
f'| Native full query, including query compilation/collection | {totals["native_query_including_compile_seconds"]:.6f} |',
f'| GPU cold query total, including all preparation | {totals["gpu_cold_query_seconds"]:.6f} |',
f'| Worker launch / static shader and buffer setup | {totals["worker_setup_seconds"]:.6f} |',
f'| Original table scan / filtering / typed encoding | {totals["table_prepare_seconds"]:.6f} |',
f'| Binary transfer + dispatch + results | {totals["roundtrip_seconds"]:.6f} |',
f'| Host result binding reconstruction | {totals["decode_seconds"]:.6f} |',
f'| GPU device work, subset of roundtrip | {totals["gpu_device_seconds"]:.6f} |', '',
'Cold total also includes the small static plan compilation. Worker setup is paid once per rule here, not once per original saturation iteration. Native query timing includes query compilation, callback collection and canonicalization; it is not a pure Join timer. Common graph clone/action-sink preparation is excluded from both query paths and separately reported.', '',
'The cold GPU path includes worker startup and table preparation; compare its total with the complete native query, not the device timer alone. The device number includes index construction, joining, guards and active-match scanning; no proposal reduction. It is not isolated join-kernel time. Do not compare these one-state query totals with the packet wall time: the packet executes an evolving graph over 1,017 iterations.', '',
'## Queries, ordered by native query time','',
'| Rule (first source line) | Bindings | Native ms | GPU cold ms | GPU transfer/dispatch/decode ms |', '|---|---:|---:|---:|---:|']
for x in sorted(r['rules'],key=lambda x:x['native_query_including_compile_seconds'],reverse=True):
 label=x['rule'].splitlines()[0].replace('|','\\|')
 if x['guards']:label+=' / '+', '.join(line.strip() for line in x['rule'].splitlines()[1:] if '<' in line or '>' in line)
 if '(= cost1 cost2)' in x['rule']:label+=' / equal costs'
 lines.append(f'| `{label}` | {x["matched_bindings"]:,} | {1000*x["native_query_including_compile_seconds"]:.3f} | {1000*x["gpu_cold_query_seconds"]:.3f} | {1000*x["gpu_resident_compute_and_return_seconds"]:.3f} |')
lines += ['', '## Coverage and equality','', '| Measure | Result |','|---|---|', f'| Native/GPU complete query equality | {r["rule_count"]}/{r["rule_count"]}, {r["matched_bindings"]:,} complete bindings |','| False negative / false positive in those queries | 0 / 0 |', '| Certified Cell shortcut coverage | Not measured; no Cell admission in this diagnostic |','| Expr / e-class / constructor coverage | Not measured; do not infer it from raw-table query equality |','| Whole GPU saturation / extraction quality | Not run; no end-to-end GPU speedup claimed |', '',f'Bulk upload {totals["uploaded_bytes"]:,} bytes; download {totals["downloaded_bytes"]:,} bytes. Small JSON controls, static program metadata and framing are excluded. No online solver is used. These totals include repeated tables across rules; the benchmark exposes rather than hides this cost.', '', 'The historical production pass profile separately splits RHS time from search, but it is not substituted for this fresh packet measurement. See README for the exact scope and reproduction.']
(a.output/'RESULTS.md').write_text('\n'.join(lines)+'\n')
