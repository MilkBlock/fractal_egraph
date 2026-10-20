# Performance order for full Pass-3 Cell-assisted execution

Reference: the validated saturation-first run in `results.json` (32.658 s).
These are affected costs, not independent predicted speedups; they overlap.
Keep the original schedule and exact matching. No sampling, candidate budget or
silent fallback is an optimization in this list.

1. **Sparse table-specific indexes - implemented.** Remove the event × all-index layout and
   all-index bucket allocation (7.39 GB device buffers). Store links only for
   indexes belonging to the event's table; size bucket segments per table.
   Targets memory traffic/maintenance within the 15.17 s device work.
2. **Shape-specific query kernels - implemented (69 exact shapes).** Stop giving every rule the largest rule's
   stack (24 atoms, 37 slots). Compile exact query-shape groups while sharing
   native rows/indexes and preserving a single commit. Same 15.17 s cost pool.
3. **Correct reusable single-Run Cell plans - unresolved, kept off.** The attempted 6.8 s improvement
   failed graph and extraction checks. Isolate its first divergence; retain the
   experiment off unless whole-graph and Tiger gates pass. Never apply a
   saturation certificate to an ordinary Run implicitly.
4. **Reduce device round trips - classification readback removed.** Avoid empty tier launches and unnecessary
   readbacks in the 4,059 dispatches. About 4.68 s of roundtrip time lies outside
   device time; no claim that all of it is removable.
5. **Compact output bindings / shared contexts - width packing implemented.** Avoid returning maximum-width
   records and repeated source/context coordinates (421 MB downloaded). Preserve
   every binding and exact native action order. Measure after earlier changes.
6. **Expand certified Cell coverage - pending proof and workload evidence.** Reuse more guaranteed port programs only
   after the remaining rule/merge environment is proved. Current full run uses
   12 parameterized dataflow closures, not precomputed fresh-DAG replay.

Implementation order is the list above, with correctness failures treated as
prerequisites, not waived to satisfy the ordering. Persist exact control states
so later candidate runs can reuse unchanged CPU/GPU-off results; keep real
snapshot, binary/worker fingerprints and stage-specific checks in the benchmark
skill. Do not repeat unchanged controls solely to generate another date.

## Current verification

The first complete layout run took 22.585 s versus the saved 32.658 s reference.
GPU buffers fell from 7.391 GB to 0.726 GB; returned bindings fell from 420.862 MB
to 65.637 MB. Exact same-commit CPU/GPU graphs, Cell-on/off graphs, and actual
Tiger CPU/GPU selected DAGs passed. The pre-existing fused-native differences
remain. No fast-but-failing single-Run experiment was enabled.

Index changes and shape grouping affect the same device work; there is no
separate speedup attributed to each without an ablation. Repeated coordinate
payload sharing under item 5 has not been implemented. Under item 3, the large
counterexample remains a blocker, not an invitation to weaken the graph gate.
The new exact control cache makes subsequent single-candidate ablations possible
without re-running unchanged CPU lanes.

## Remaining priorities from the fresh ruleset timing diagnostic

The single-candidate/control-reuse validation took 24.863 s with per-ruleset
accounting, versus the 22.585 s timing run above. Both passed identical graph
and same-commit Tiger gates. These are separate recorded runs, not an average;
we do not attribute their entire difference to timer overhead. Query accounting
is now opt-in via `CELL_QUERY_TIMINGS=1`.

| Ruleset | Query seconds (including device) | Device seconds | Complete bindings |
|---|---:|---:|---:|
| always-run | 6.684 | 4.630 | 2,407,793 |
| type-analysis | 1.441 | 1.171 | 108,640 |
| loop-iters-analysis | 0.700 | 0.595 | 87 |
| all-optimizations (combined) | 0.678 | 0.533 | 863 |
| state-edge-passthrough | 0.640 | 0.634 | 4 |

The first semantic target remains `always-run`, but its failed Cell Run proof
must be repaired before enabling that path. Next are type-analysis and the
sparse boundary queries. These counts are complete bindings, not index probes.
Do not add query time and device time, or call these per-rule measurements.
