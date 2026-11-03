# Raytrace Pass 3: RHS packets did not give a net speedup

Measured 2026-10-30, implementation `42883fc` (reports at `5738d09`).
Actual complete resident GPU runtime; source markers **off**, existing generic
multi-stage Cells/private schedules **on**. Same 1,048,576-row and 262,144-match
capacities. This is not the older hybrid ~12-second backend.

| Clean measurement | Historical control | RHS packets |
|---|---:|---:|
| Inference (compile/extraction excluded) | 162.650079 s | **164.409063 s** |
| GPU command time | 160.491739 s | 162.025303 s |
| Outer command buffers | 2,137 | **2,392** |
| Allocated engine storage (not RSS) | 1,507,664,840 B | 1,522,608,168 B |

This single measurement is **1.08% slower**, effectively no demonstrated net
improvement. The control is historical, not a fresh same-binary pair; do not
interpret the small difference as a statistically established regression.
Storage grew by 14.25 MiB. All four Cargo profile stages subsequently hit cache.

## Correctness and comparable work

- The typed programs differ only by redundant `Sequence` wrappers. Flattening
  those wrappers preserves `Repeat`, `Saturate`, and every Run boundary.
- Compiled control tapes, private-stage programs, initial rows/sets, table
  schemas, strings, anchors, and action bytecode are identical.
- Both finish **9,786 Runs**, **36 private regions / 782 private Runs**.
- Entire typed graph comparison passes: **702,038 physical rows**, **524,742
  e-classes across all sorts**, identical facts, function values, equality
  partition and visibility. These are not Expr-only node/class counts.
- Physical row ordering, class IDs and fact IDs change. The existing anchored
  typed-key comparator proves a bijection without native inference/rebuild
  repairing either graph. Watermarks, Run metadata/history and private work are
  identical. Extraction was not rerun.
- The instrumented replay exactly reproduces the new clean executor's complete
  raw state/history, including its new allocation IDs and work counters.
- Matcher outputs remain **3,380,944**; total rule invocations remain
  **4,068,206**. This optimization did not expand Cell coverage or skip additional
  matching.

## What the new publisher actually handled

- 600,223 bindings entered packet cohorts (includes no-op bindings).
- 690,327 active construction requests; 238,119 fresh rows.
- 414,495 existing-key hits and 37,713 in-batch duplicate requests. These are
  hashcons/deduplication, **not** RipSatCell cache hits.
- 26,767 Cell contexts / 53,534 recipe steps; 65,198 ordered residual invocations.
- 728 dependency passes; zero preparation fallbacks; 260 host quanta dispatched
  the packet pipeline. The maximum packet arena grew to 131,072 requests.

A substantial part replaces work that was already parallel:

| Fresh rows published in parallel | Before | After |
|---|---:|---:|
| Legacy independent constructor writer | 114,287 | 0 |
| Existing Cell effect waves | 82,890 | 77,181 |
| New RHS packets | 0 | 238,119 |
| Total | **197,177** | **315,300** |

Net additional fresh rows moved to parallel publication: **118,123**, not
238,119. Existing wave-validated steps fall by exactly 53,534, matching the new
packet Cell steps. The wave count only falls from 3,304 to 3,300: packaging many
short cones did not remove most remaining waves/dependencies.

The old writer omitted some path-level `direct_single_step_invocations`
accounting now credited by the new writer. Do not interpret that counter's
increase as more semantic work; matcher and total invocation counts are equal.

## Why there is no net gain

Encoder diagnostics are separate runs: **200.597 → 203.100 s**. They are not a
breakdown of the clean 162.65/164.41-second measurements.

| Diagnostic encoder group | Before | After |
|---|---:|---:|
| RHS publication, all publishers | 121.609 s | 120.760 s |
| **`batched_cell_publish` alone** | **120.847 s** | **119.990 s** |
| Seed + match | 12.551 s | 13.073 s |
| Index/control preparation | 9.409 s | 10.263 s |
| Rebuild/compaction | 9.599 s | 10.212 s |
| Parallel Cell waves | 1.268 s | 1.645 s |

The dominant serial publisher barely changed. The added 255 outer command
buffers and other control/preparation work accompany the small publication
saving. Readiness gating can defer a newly found cohort to the next existing
host checkpoint; it avoids idle dispatches but does not eliminate scheduling
cost on this large workload. This is not a claim that all 255 extra commands or
all timing differences have one isolated cause.

The most useful new observation is that several very slow publisher blocks have
**few final bindings**, and dispatch no packet kernels:

| Run (`always-run`) | Final Run bindings | Publisher encoder time within one command |
|---|---:|---:|
| 9,706 | 91 | 1.463 s |
| 9,184 | 331 | 1.449 s |
| 9,180 | 556 | 1.434 s |
| 9,704 | 1,010 | 1.468 s |

Each selected command starts and ends within the same unfinished Run. Time
includes publisher maintenance; it is **not** a measured per-RHS action cost.
This makes global maintenance inside publication a more useful investigation
than simply widening construction admission.

A concrete source-level candidate is continuation after rebuild: the resident
compactor invalidates the row index; `bc_finish_run` can enter state 2 directly;
the state-2 preparation branch does not run ordinary parallel index preparation.
A later `row_lookup` can reach serial `ensure_row_index` inside publication.
There are 128 continuation blocks. This is a **code-backed candidate, not an
isolated timing proof**. The aggregate 1,470 index-rebuild counter includes other
paths and cannot be read as 1,470 serial rebuilds. The next diagnostic should
separate inline index refresh, custom merge, actual action execution and receipt
maintenance inside the expensive publisher.

No new hardware occupancy/bandwidth capture was made. The synthetic 5.86×
nested-RHS result does not transfer to this workload.

## Reproduce

```sh
cargo profile --case raytrace
python3 experiments/rhs_packets/raytrace_report.py
# Later, inspect without rerunning compiler/GPU:
cargo profile --case raytrace --cached-only
```

[Machine-readable comparison and provenance](raytrace.json). The comparison
script uses the retained `out/resident-restoration-pass3` control. GPU state,
compiled workers and full profiles remain in `out/`; they are not Git artifacts.
