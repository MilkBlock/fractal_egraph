# Shared table runtime: real raytrace result

One release run per lane on Apple M3 Max, one native thread. Actual saved production Pass-3 packet; original rules and schedule preserved. Not a complete compiler/extraction run. OS and Metal caches are not flushed; no averages.

| Lane | Setup s | Run s | Setup + run s |
|---|---:|---:|---:|
| Native original fused search/apply | prefix already loaded | 7.964195 | 7.964195 |
| CPU native queries, sorted commit | 0.001893 | 8.517649 | 8.519542 |
| GPU shared queries, same sorted commit | 0.077886 | 2.723649 | 2.801536 |

The **same-commit** CPU/GPU comparison passes exact whole-graph equality: **3.13×** run speedup, **3.04×** including preparation. The CPU lane executes cached native semi-naive queries; it is not a brute-force simulator. Its callback collection and sorted commit are measured. The original fused native lane is retained separately, not relabeled as the same-commit control or pristine.

| GPU execution component | Seconds |
|---|---:|
| Native event ingest / shared store | 0.143240 |
| Packet encoding | 0.010733 |
| GPU round trips | 1.144754 |
| Device work, subset of round trips | 0.633258 |
| Decode/sort | 0.028781 |
| Original RHS + merge/rebuild | 1.378602 |

Native original search+apply / insert-merge / rebuild: 6.825080 s / 0.047507 s / 1.047155 s. Search+apply includes RHS staging. Device time must not be added to roundtrip time.

## Sharing and reclamation

| Measure | Result |
|---|---:|
| Workers / program installations | 1 / 1 |
| Shared native tables | 12 |
| Batch RPCs | 854 |
| Live rows / allocated row slots | 242,748 / 242,748 |
| Reused row slots | 48,376 |
| Index-log compactions | 2 |
| Retained historical matches | 0 |
| Explicit device buffers | 103.24 MiB |
| Bulk upload / download | 35,643,420 / 17,249,940 bytes |

Historical per-rule implementation: 11.276 s run, 2,673 batch RPCs, 444.40 MiB summed worker buffer peaks. These are separately measured historical results, not an isolated one-change speedup. Device buffers are not process RSS and the original native egraph remains explicit.

## Correctness

| Check | Result |
|---|---|
| GPU vs CPU with identical commit policy | exact: 657,016 public rows; facts, primitive values, visibility and equality |
| Independent native frontier checks, separate run | 3,600 passed |
| Native handback unchanged | True |
| Original fused engine exact graph | False |
| Original fused engine cost comparison | True, 6,985 Expr keys |

Original-path difference: different final table counts: [("Smaller", Some(72049), Some(73234))]

The original-path gate remains visible. Different ordering in the original fused path can choose different terms through custom merge, so its timing is not claimed as a strict same-graph speedup. The same-commit CPU reference reproduces the GPU graph entirely; no Smaller rows or selected fields are excluded from that primary comparison.

| Sort | Original native nodes/classes | Shared GPU nodes/classes |
|---|---:|---:|
| Expr | 69,455 / 23,155 | 69,455 / 23,155 |
| Term | 62,749 / 62,749 | 62,749 / 62,749 |
| TermAndCost | 134,798 / 62,749 | 135,983 / 62,749 |

No baked Cell shortcut coverage is claimed: all original LHS atoms are represented. The checked run is excluded from timing. High-water buffer capacity remains allocated, but historical matching records are eliminated and event history is compacted without changing eligibility.

Raw reports: `/Users/mineralsteins/.codex/worktrees/cell-assisted-egglog/egg_layout/out/shared-family-bench-final/report.json` and `/Users/mineralsteins/.codex/worktrees/cell-assisted-egglog/egg_layout/out/shared-family-checked-final/report.json`. Source, input, executable and worker hashes are retained in `results.json`.
