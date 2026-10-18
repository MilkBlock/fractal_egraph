# Cell projection results

Actual egglog execution, original isolated eggcc If/util rules, 300 held-out contexts (10% min/max and 90% near misses). One execution per lane on an Apple M3 Max; no averaging. This is not the previous full raytrace packet or whole eggcc. A frozen analyze library supplies the positive Cell shapes.

## Matching work (separate instrumented run)

| Lane | Entered Join stages | Certification key lookups | Stages served from a Cell |
|---|---:|---:|---:|
| full_gpu | 3,842,672 | 0 | 0 |
| cell_order_only | 4,594,172 | 0 | 0 |
| cell_certificates | 3,452,372 | 1,530 | 1,140,600 |

With the **same query order**, Cell reuse reduces entered stages by 24.85%; including its extra certification lookups, the reduction is 24.82%. Against the original GPU order, the net reduction is 10.12%. Ordering alone increases work on this sample. These counts are not row candidates or storage compression.

The library yields 4 shared projections. Their actual device cache is 3,538,944 bytes (3.38 MiB); native tables remain explicit. 2,400 generation checks reuse existing certificates without probing keys. Guards and boundary joins remain ordinary exact computations.

Examples are `If(pred, inputs, thn, els)` exposing the already established `InIf(true, pred, inputs)` and `InIf(false, pred, inputs)` contexts, and the `Bop`/`LessThan` operator check. The actual projected atoms and catalog state IDs are retained in the JSON. This does not assume a generic If has either branch context: missing or changed witnesses fall back.

## Time (counters and audit disabled)

| Lane | Setup s | Run s | Device s, included in run |
|---|---:|---:|---:|
| Native cached-query CPU, same commit | 0.001234 | 0.009584 | — |
| full_gpu | 0.046616 | 0.173639 | 0.063161 |
| cell_order_only | 0.043097 | 0.093232 | 0.071974 |
| cell_certificates | 0.049775 | 0.093201 | 0.071840 |

This small case is still faster on native CPU. Host round-trip/lazy first-dispatch costs dominate these short GPU timings; a single wall-time ratio is **not** a demonstrated throughput improvement. The established result is exact reuse of internal matching work. Do not multiply this ratio by the earlier raytrace speedup.

## Correctness

All three GPU variants equal the CPU control on **4,868** final public rows, values, visibility and equivalence. A separate run passes **27** complete native-frontier checks. Native RHS, merge, rebuild and original schedule boundaries are retained.

Focused GPU tests also cover external-only deltas, unknown shapes, late union alternatives, subsume, row-slot recycling, index compaction, two Cell segments/backtracking, numerical guards and sharing across consumers. No candidate or match count is truncated.

Reverse/enumerating reads remain the full query. Cell readiness is currently validated from keyed committed rows, not inferred solely from producer history. Certificate generation checking still scans the row/plan cache after changes; per-dependency wake lists remain future work. See README for time/space bounds.
