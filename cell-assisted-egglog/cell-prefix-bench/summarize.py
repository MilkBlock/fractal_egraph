#!/usr/bin/env python3
"""Keep timing, work counters, and independent correctness checks separate."""
import argparse,hashlib,json
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('timed',type=Path);p.add_argument('checked',type=Path);p.add_argument('profiled',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
raw=[json.loads(f.read_text()) for f in [a.timed,a.checked,a.profiled]]
t,c,w=raw
assert not t['audit'] and not t['profile'] and c['audit'] and w['profile']
for r in raw:
    assert r['complete']
    for x in r['variants']:assert x['verification']['exact_rows_values_equivalence_visibility']
for r in raw[1:]:
    left=t['provenance']['hashes'];right=r['provenance']['hashes']
    for k in left.keys() & right.keys():assert left[k]==right[k],k
    assert next(v for k,v in left.items() if k.endswith('/heldout.egg'))==next(v for k,v in right.items() if k.endswith('/heldout.egg'))
    assert [x['verification'] for x in t['variants']]==[x['verification'] for x in r['variants']]
timed={x['name']:x['report'] for x in t['variants']};work={x['name']:x['report']['cell_work'] for x in w['variants']}
audit=sum(q['stats']['audit_queries'] for v in c['variants'] for q in v['report']['rules'])
assert audit>0
out=a.output;out.mkdir(parents=True,exist_ok=True)
report={'timed':t,'checked':c,'profiled':w,'raw_reports':[{'path':str(f.resolve()),'sha256':hashlib.sha256(f.read_bytes()).hexdigest()} for f in [a.timed,a.checked,a.profiled]]}
(out/'results.json').write_text(json.dumps(report,indent=2)+'\n')
lines=['# Cell projection results','',
'Actual egglog execution, original isolated eggcc If/util rules, 300 held-out contexts (10% min/max and 90% near misses). One execution per lane on an Apple M3 Max; no averaging. This is not the previous full raytrace packet or whole eggcc. A frozen analyze library supplies the positive Cell shapes.','',
'## Matching work (separate instrumented run)','',
'| Lane | Entered Join stages | Certification key lookups | Stages served from a Cell |','|---|---:|---:|---:|']
for n,x in work.items():lines.append(f'| {n} | {x["join_probes"]:,} | {x["certification_probes"]:,} | {x["skipped_join_probes"]:,} |')
base=work['cell_order_only']['join_probes'];cell=work['cell_certificates'];net=cell['join_probes']+cell['certification_probes']
full=work['full_gpu']['join_probes'];cache=timed['cell_certificates']['device']['cell_cache_bytes']
lines += ['',f'With the **same query order**, Cell reuse reduces entered stages by {100*(1-cell["join_probes"]/base):.2f}%; including its extra certification lookups, the reduction is {100*(1-net/base):.2f}%. Against the original GPU order, the net reduction is {100*(1-net/full):.2f}%. Ordering alone increases work on this sample. These counts are not row candidates or storage compression.', '',
f'The library yields {len(timed["cell_certificates"]["cells"]["plans"])} shared projections. Their actual device cache is {cache:,} bytes ({cache/2**20:.2f} MiB); native tables remain explicit. {cell["certificate_reuses"]:,} generation checks reuse existing certificates without probing keys. Guards and boundary joins remain ordinary exact computations.', '',
'Examples are `If(pred, inputs, thn, els)` exposing the already established `InIf(true, pred, inputs)` and `InIf(false, pred, inputs)` contexts, and the `Bop`/`LessThan` operator check. The actual projected atoms and catalog state IDs are retained in the JSON. This does not assume a generic If has either branch context: missing or changed witnesses fall back.', '',
'## Time (counters and audit disabled)','',
'| Lane | Setup s | Run s | Device s, included in run |','|---|---:|---:|---:|',
f'| Native cached-query CPU, same commit | {t["cpu"]["runtime"]["setup_seconds"]:.6f} | {t["cpu"]["runtime"]["run_seconds"]:.6f} | — |']
for n,x in timed.items():
    s=x['runtime'];lines.append(f'| {n} | {s["setup_seconds"]:.6f} | {s["run_seconds"]:.6f} | {s["device_seconds"]:.6f} |')
lines += ['','This small case is still faster on native CPU. Host round-trip/lazy first-dispatch costs dominate these short GPU timings; a single wall-time ratio is **not** a demonstrated throughput improvement. The established result is exact reuse of internal matching work. Do not multiply this ratio by the earlier raytrace speedup.','',
'## Correctness','',
f'All three GPU variants equal the CPU control on **{t["variants"][0]["verification"]["rows"]:,}** final public rows, values, visibility and equivalence. A separate run passes **{audit}** complete native-frontier checks. Native RHS, merge, rebuild and original schedule boundaries are retained.', '',
'Focused GPU tests also cover external-only deltas, unknown shapes, late union alternatives, subsume, row-slot recycling, index compaction, two Cell segments/backtracking, numerical guards and sharing across consumers. No candidate or match count is truncated.', '',
'Reverse/enumerating reads remain the full query. Cell readiness is currently validated from keyed committed rows, not inferred solely from producer history. Certificate generation checking still scans the row/plan cache after changes; per-dependency wake lists remain future work. See README for time/space bounds.']
(out/'RESULTS.md').write_text('\n'.join(lines)+'\n')
print(out/'RESULTS.md')
