# Current optimization priority after Cell query reuse

Latest sat-skip work: [certified comparison closure](SAT_COMPARISON.md).
On the final same-binary ablation the joint handler fell from 3.329 to 2.074 s,
and inference from 18.634 to 17.708 s. This closes the inner comparison fixed
point; producer/helper dataflow and broader cross-rule query sharing remain.

Follow-up: all eight pools were addressed in
[the individual optimization report](INDIVIDUAL_OPTIMIZATIONS.md).
Current inference is 21.265 s; device time is 6.280 s. Keep the next priorities
as cross-rule partial-query sharing, native rebuild (2.689 s), and joint
closure dataflow (3.849 s). Compact binding workspaces and staged guards are
implemented; cross-rule fragment materialization is still open. Worker
encoder batching and index-buffer reuse did not demonstrate a net gain and
were reverted. Cold shader setup is an additional separate issue.

The investigation and timing pools below describe the previous baseline.

These priorities replace the older pre-Cell ranking. The current ordinary
run took 22.718 s. A separate diagnostic split GPU command buffers to distinguish
maintenance from matching. That diagnostic is not a speed benchmark. Saved
measurements and checks: [priority-profile.json](priority-profile.json).

| Priority | Remaining work | Measured cost pool | Next concrete direction |
|---|---|---:|---|
| 1 | Shared partial queries and Cell boundary matching | GPU matching 7.29 s | Start with residual `always-run` (2.65 s) and `type-analysis` (1.17 s). Reuse common prefixes/ports across different LHSs, retaining their separate binding and guard constraints. |
| 2 | Native effect submission and rebuild | 4.29 s, including rebuild 2.76 s | Reduce temporary worklists, copies and repeated canonicalization. Do not omit RHSs or change merge/commit order. |
| 3 | Parameterized joint Cell closure execution | 3.74 s | Reduce repeated internal port traversal and effect materialization within the existing certified boundaries. The 12 boundaries use dataflow execution, not fresh-DAG replay. |
| 4 | Commit/event ingestion | 2.24 s | Share payload ownership between the Cell observer and GPU mirror; remove redundant copying/synchronization without losing union, visibility or row versions. |
| 5 | Host/worker overhead outside device execution | about 2.27 s | Reduce packet preparation and waiting while preserving each original Run boundary. Do not merge independently observable Runs. |
| 6 | Stable structural tie-key maintenance | 1.57 s | Maintain the existing stable-choice adapter incrementally; do not remove or change the tie policy to obtain speed. |
| 7 | Cell frontier construction | 0.94 s | Batch cached port reads and reduce allocation for the 1.92M bindings. Larger Cell coverage is worthwhile only if it saves more search than maintenance. |
| 8 | GPU index/table maintenance | 0.13 s | Deprioritize further construction/clearing optimization. This says nothing about the quality of indexes used during matching. |

The GPU matching pool includes candidate routing and binding computation, not
just Join probes. Ruleset binding totals include Cell-provided bindings; they are
not a count of GPU probes or of GPU-only matches. Index maintenance is billed to the next consumer ruleset, not
necessarily to the rule that produced its changes. Subcomponents overlap their
parent pools; these costs are not additive promised savings.

## What changed in this iteration

- `CELL_QUERY_TIMINGS=1` now separates device maintenance and matching, and the
  benchmark refuses to publish a speedup for this split-command diagnostic.
- Exact whole-query sharing was implemented and checked: identical typed LHS,
  all guards, slot wiring and current native watermark must agree. RHSs still
  submit independently in the original order. It is an opt-in experiment via
  `CELL_SHARE_IDENTICAL_QUERIES=1`, not enabled by default.
- A per-Run active-anchor routing experiment passed correctness but did not
  improve end-to-end time, so it was removed from the default implementation.
  Its reproducible source delta is [active-query-routing.patch](active-query-routing.patch).

| Experiment | Inference | Device | Outcome |
|---|---:|---:|---|
| Current Cell frontiers, ordinary ruleset timers | 22.718 s | 7.304 s | Reference for this investigation |
| Active-anchor routing only | 22.942 s | 7.167 s | No meaningful net gain; default reverted |
| Active routing + exact whole-query sharing | 22.505 s | 7.188 s | 24,947 bindings shared; no clear device-time improvement |

Each configuration ran once. Do not average these runs or attribute all timing
variation to the code change. The shared variant was timed on top of the active
routing candidate; its standalone speedup has not been measured. There are 512
exact query groups among 547 rules, so whole-LHS equality alone covers little
of the remaining search. The larger opportunity is common **partial** structure.

Both experiments preserved complete same-commit CPU/GPU graphs, the Cell-toggle
graph gate and actual Tiger CPU/GPU output. The pre-existing fused-native
execution/output difference remains; no pristine-equivalent replacement claim
is made.

The completed sparse-index layout, exact shape kernels, packed bindings and
correct read-only Cell frontier protocol remain in place. Their earlier
measurements are in [RESULTS.md](RESULTS.md) and [CELL_QUERY_FIX.md](CELL_QUERY_FIX.md).
