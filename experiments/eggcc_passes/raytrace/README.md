# Raytrace: production pass timing

One measured optimization on 2026-10-07 (Asia/Shanghai). Input:
`check_statewalk/benchmarks/passing/raytrace/raytrace.rs` (29 functions).
Cached production profiler: egglog ebba7bb / experimental 08771f9,
parallel schedule, default context mode, actual Tiger extraction, no ILP.
Source and binary SHA-256 hashes are in `timing.json`.

## Time (seconds)

| Pass | Execution including setup | Search/binding | RHS | Insert/merge | Rebuild | Extraction wall | Serialization |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1 | 4.939 | 3.805 | 0.087 | 0.067 | 0.625 | 1.342 | 0.308 |
| 2 | 13.397 | 10.875 | 0.181 | 0.105 | 1.682 | 3.785 | 0.887 |
| 3 | 24.287 | 18.882 | 0.406 | 0.319 | 3.869 | 3.800 | 0.917 |
| Total | 42.623 | 33.562 | 0.674 | 0.490 | 6.177 | 8.927 | 2.112 |

Search/binding accounts for 78.74% of execution; rebuild 14.49%,
RHS 1.58%, insert/merge 1.15%. Execution + extraction + serialization
is 53.662 seconds, NOT complete CLI wall time. Source generation, frontend,
other bookkeeping and final output are outside this sum.
Search includes matching management, not exclusively Join probes.
No per-rule ranking was collected, so this does not identify a specific slow rule.

| Pass | Serialized nodes (all sorts) | Serialized e-classes | Logical matches |
|---|---:|---:|---:|
| 1 | 247299 | 180802 | 924153 |
| 2 | 653093 | 427739 | 1767688 |
| 3 | 716893 | 543572 | 3093789 |

Each pass uses a new graph seeded from the previous pass's extracted program;
these are not cumulative graph counts. Matches are not committed mutations.

## Output rendering trap

The initial runner used `dag-optimize`. After all three passes completed and
saved their timing records, that mode rendered the optimized program using
Graphviz `dot`. The dot subprocess spent over one minute on this unrelated
output. It was terminated, so the overall command exited 101 during rendering.
All three measured passes and their extraction had already completed. Do not
report this run as a successful end-to-end command or use its total wall time.

The runner now uses `optimized-pretty-print`: the same `dag_in_context::optimize`
entry, with textual output instead of Graphviz. This output path was smoke-tested
on fact; raytrace was not timed a second time.

## Frozen-library coverage

| Pass | Expr nodes | Expr e-classes | Constructor rows | Complete LHS |
|---|---|---|---|---|
| 1 | unavailable | unavailable | unavailable | unavailable |
| 2 | not run | not run | not run | not run |
| 3 | not run | not run | not run | not run |

Separate diagnostic used the frozen fact Pass-1 library (163 observation patterns,
7402 signatures). Pass-1 diagnosis was stopped after over five minutes, at the
last reported 1400 native steps; no validated final report was produced. No
coverage or native-control equivalence is claimed. Diagnostic time is excluded
from production timing. No analyze, trace recapture for bake, or new bake was run.

## Reproduce

```sh
python3 skills/eggcc-pass-benchmark/scripts/bench.py run \
  --source ../check_statewalk/benchmarks/passing/raytrace/raytrace.rs \
  --output out/raytrace-pass-repeat
```

This invokes all three passes once, followed by the separate (potentially slow)
coverage audit. Raw records from this measurement: `out/eggcc-raytrace-pass-audit`.
