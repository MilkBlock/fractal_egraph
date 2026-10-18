#!/usr/bin/env python3
"""Summarize separate timing, oracle, and work-counter runs without averaging."""
import argparse,hashlib,json
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('timed',type=Path);p.add_argument('checked',type=Path);p.add_argument('profiled',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
paths=[a.timed,a.checked,a.profiled];t,c,w=[json.loads(p.read_text()) for p in paths]
assert not t['audit'] and not t['profile'] and c['audit'] and w['profile']
for r in [t,c,w]:
    assert r['complete'] and len(r['variants'])==4
    assert all(x['verification']['exact_rows_values_equivalence_visibility'] for x in r['variants'])
for r in [c,w]:
    assert [v['verification'] for v in t['variants']]==[v['verification'] for v in r['variants']]
    left=t['provenance']['hashes'];right=r['provenance']['hashes']
    for k in left.keys() & right.keys():assert left[k]==right[k],k
    assert next(v for k,v in left.items() if k.endswith('/heldout.egg'))==next(v for k,v in right.items() if k.endswith('/heldout.egg'))
timed={v['name']:v['report'] for v in t['variants']};work={v['name']:v['report']['cell_work'] for v in w['variants']}
checks=sum(q['stats']['audit_queries'] for v in c['variants'] for q in v['report']['rules'])
assert checks>0
out=a.output;out.mkdir(parents=True,exist_ok=True)
(out/'results.json').write_text(json.dumps({'timed':t,'checked':c,'profiled':w,'raw_reports':[{'path':str(p.resolve()),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in paths]},indent=2)+'\n')
lines=['# Commit-driven Cell interface results','',
'Real native egglog schedules and RHS with the original isolated eggcc If/util rules. Frozen analyze library, 300 held-out contexts, 10% min/max and 90% near misses. Apple M3 Max; one execution per lane, no average. This is not the full raytrace packet or whole eggcc.', '',
'## Time and storage','', '| Lane | Setup s | Run s | Device s (inside run) | Allocated GPU buffers MiB |','|---|---:|---:|---:|---:|',
f'| Native cached-query CPU, same commit | {t["cpu"]["runtime"]["setup_seconds"]:.6f} | {t["cpu"]["runtime"]["run_seconds"]:.6f} | — | — |']
for n,r in timed.items():
    s=r['runtime'];lines.append(f'| {n} | {s["setup_seconds"]:.6f} | {s["run_seconds"]:.6f} | {s["device_seconds"]:.6f} | {r["device"]["device_bytes"]/2**20:.3f} |')
lines+=['','`cell_cached` is the preceding implementation; `cell_certificates` is now the event-driven path. Buffer totals are explicitly allocated steady-state device buffers, not process RSS, transient resize peaks, or a Cell compression ratio. Cold dispatch and host round trips make these short single-run wall times variable. **Do not claim a new throughput speedup from these ratios. Native CPU is still faster on this small case.**','',
'## Work (separate instrumented execution)','', '| Lane | Ordinary Join probes | Port Join probes | Certification probes | Direct port bindings |','|---|---:|---:|---:|---:|']
for n,r in work.items():lines.append(f'| {n} | {r["join_probes"]:,} | {r["port_probes"]:,} | {r["certification_probes"]:,} | {r["port_bindings"]:,} |')
x=work['cell_certificates'];old=work['cell_cached']
lines+=['',f'The event path publishes {x["ports_published"]:,} ready interfaces and processes {x["dirty_cells"]:,} dirty interface tasks. The prior path performs {old["certificate_reuses"]:,} successful repeated generation checks; the event path does not rescan those certificates. It retains {x["missing_key_waits"]:,} waits for absent facts/equality conditions.', '',
f'**This workload produces {x["port_bindings"]} accepted matches directly from ready-port deltas.** Its selected If/InIf interfaces are largely ready at bootstrap; later useful matches are external-delta driven. The ordinary Join count therefore remains {x["join_probes"]:,}, the same as the cached lane. Extra watch/port state costs memory. This is an honest limitation of the current projection/driver selection, not evidence of successful Cell-wide search elimination.', '',
'## Correctness and exercised transitions','',
f'Every GPU lane equals the CPU control on all **{t["variants"][0]["verification"]["rows"]:,}** public rows, values, visibility and equivalence. The separate checked execution passes **{checks}** complete native-frontier comparisons.', '',
'The focused transition tests additionally exercise cases absent from this If workload: a native helper commit waking a missing-key wait; direct port matching with an old external boundary; numerical rejection and output-buffer growth; retaining prefix bindings; a ready parent creating a child whose port drives the next stage; and bounded watch/port compaction with a delayed consumer and a stateful merge counter. Tests reject missing, duplicate and replayed matches.', '',
'This implements analyzed **interface-state** transitions. Arbitrary whole-RipSatCell graft execution, multivalued reverse projections and replacement of the original RHS are not claimed. See README for the ownership proof and remaining costs.']
(out/'RESULTS.md').write_text('\n'.join(lines)+'\n')
print(out/'RESULTS.md')
