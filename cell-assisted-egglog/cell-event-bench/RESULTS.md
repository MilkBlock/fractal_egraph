# Commit-driven Cell interface results

Real native egglog schedules and RHS with the original isolated eggcc If/util rules. Frozen analyze library, 300 held-out contexts, 10% min/max and 90% near misses. Apple M3 Max; one execution per lane, no average. This is not the full raytrace packet or whole eggcc.

## Time and storage

| Lane | Setup s | Run s | Device s (inside run) | Allocated GPU buffers MiB |
|---|---:|---:|---:|---:|
| Native cached-query CPU, same commit | 0.001241 | 0.008844 | — | — |
| full_gpu | 0.323398 | 0.123262 | 0.065365 | 3.593 |
| cell_order_only | 0.203732 | 0.092088 | 0.067545 | 3.596 |
| cell_cached | 0.143322 | 0.173965 | 0.067201 | 6.971 |
| cell_certificates | 0.059248 | 0.091559 | 0.066718 | 20.096 |

`cell_cached` is the preceding implementation; `cell_certificates` is now the event-driven path. Buffer totals are explicitly allocated steady-state device buffers, not process RSS, transient resize peaks, or a Cell compression ratio. Cold dispatch and host round trips make these short single-run wall times variable. **Do not claim a new throughput speedup from these ratios. Native CPU is still faster on this small case.**

## Work (separate instrumented execution)

| Lane | Ordinary Join probes | Port Join probes | Certification probes | Direct port bindings |
|---|---:|---:|---:|---:|
| full_gpu | 3,842,672 | 0 | 0 | 0 |
| cell_order_only | 4,594,172 | 0 | 0 | 0 |
| cell_cached | 3,452,372 | 0 | 1,530 | 0 |
| cell_certificates | 3,452,372 | 0 | 1,530 | 0 |

The event path publishes 1,200 ready interfaces and processes 1,830 dirty interface tasks. The prior path performs 2,400 successful repeated generation checks; the event path does not rescan those certificates. It retains 30 waits for absent facts/equality conditions.

**This workload produces 0 accepted matches directly from ready-port deltas.** Its selected If/InIf interfaces are largely ready at bootstrap; later useful matches are external-delta driven. The ordinary Join count therefore remains 3,452,372, the same as the cached lane. Extra watch/port state costs memory. This is an honest limitation of the current projection/driver selection, not evidence of successful Cell-wide search elimination.

## Correctness and exercised transitions

Every GPU lane equals the CPU control on all **4,868** public rows, values, visibility and equivalence. The separate checked execution passes **36** complete native-frontier comparisons.

The focused transition tests additionally exercise cases absent from this If workload: a native helper commit waking a missing-key wait; direct port matching with an old external boundary; numerical rejection and output-buffer growth; retaining prefix bindings; a ready parent creating a child whose port drives the next stage; and bounded watch/port compaction with a delayed consumer and a stateful merge counter. Tests reject missing, duplicate and replayed matches.

This implements analyzed **interface-state** transitions. Arbitrary whole-RipSatCell graft execution, multivalued reverse projections and replacement of the original RHS are not claimed. See README for the ownership proof and remaining costs.
