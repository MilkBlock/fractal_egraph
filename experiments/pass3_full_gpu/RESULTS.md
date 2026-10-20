# Full Pass-3 GPU queries with Cell saturation first

This is the actual `cell-assisted-egglog` native event loop on the saved raytrace
Pass-3 main schedule, not a synthetic query workload. The immutable upstream
baseline tag was not changed. Timings use the local fork and its stable
`to-hoist` policy; they are not pristine upstream timings.

## Integration

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py run \
  --all-rules --joint-sat --output out/pass3-full-cells
```

`Runtime::new_full` compiles all 547 unique original scheduled rules, including
combined rulesets. A combined Run searches one frozen snapshot and commits once.
`cell-stable-keys` is our CPU-side selection adapter, not an original rule.
Original RHS actions, function merges, union, visibility changes and rebuild
remain native. Unsupported queries are errors, not silent CPU fallbacks.

The certified joint terms handler runs before GPU dispatch at its accepted
saturation boundaries. Its effects enter the shared table mirror before later
GPU queries. Missing certificates retain complete queries. This benchmark's
12 handled boundaries used exact parameterized dataflow; **zero fresh-DAG
macros fired**. Do not call all 12 boundaries constant-time saturation skips.

## Checks and single-run results

The saturation-first path passed **478,136 native incremental query checks**
across 6,890 scheduled ruleset calls. That audit includes the remaining GPU
queries; it does not audit matches already replaced by certified Cell execution.

| Path | Seconds | Same-commit CPU graph | Cell-on/off graph | Actual Tiger DAG vs same-commit CPU |
|---|---:|---|---|---|
| CPU, original fused execution | 24.482 | Different partition protocol | — | Different selected DAG |
| CPU, complete sorted-commit reference | 87.627 | Reference | — | Reference |
| All-rule GPU, Cell handler off | 34.882 | Passed through Cell-on/off comparison | Reference | — |
| All-rule GPU, certified saturation handler on | **32.658** | **Pass** | **Pass** | **Pass** |
| Experimental extra single-Run Cell programs | 25.828 | **FAIL** | **FAIL** | **FAIL** |

The validated Cell handler saves 6.4% versus the same GPU backend with the
handler off (1.068×). The complete GPU path is still slower than fused native
execution. Even the validated GPU path differs from fused native in `Smaller`,
`TCPair`, and `TermConcat` facts and in the Tiger-selected DAG. Equal costs on
initial Expr anchors do not replace these failed checks. No “faster native
replacement with unchanged extraction” claim is made.

The historical, superseded single-Run experiment invoked 518 Cell programs, built 11 local scalar
closures, and reduced device time, but introduced an additional `Smaller`
structure difference relative to the sorted-commit reference. Tiger also found
a `Concat` difference. It was disabled while this cause was investigated.
That implementation can be reproduced from commit `c7efb32`; the old
`CELL_EXPERIMENTAL_RUN_PROGRAMS` switch is no longer active. Small tests are insufficient to certify this broader dispatch
change. Ordinary Run must not be given saturation semantics implicitly.

These are the two existing measured candidates; the final default restores the
validated saturation-only behavior rather than running all unchanged controls
again. The explicit experimental toggle is included in benchmark cache identity.

## What changed

- Typed LHS compiler: exact checked i64 `+`, `%`, inequalities, typed `!=`,
  computed table keys, global bindings, and empty queries. Native side retains
  residual checks and all RHS semantics, including `subsume`/`delete`.
- GPU dispatch: each delta table row indexes its consuming query anchors.
  No row × all-rules × maximum-arity launch. Every atom occurrence is retained.
- Query order: fully bound function keys have priority; no binding is sampled.
- Cell dispatch: the existing saturation handler remains ahead of full queries.
  The additional selected-Run experiment has a separate gate and counters.

The first brute-force all-rule dispatch was manually stopped after 855 seconds;
a second diagnostic run was stopped during a long join. Both partial records are
retained under `out/`; neither is a completed benchmark or a semantic failure.

## Remaining limits

The full family still shares maximum register/depth layout and dense event-index
links. Its reported GPU buffer allocation is about **7.39 GB** (allocated buffer
bytes, not RSS). This is not yet a good memory design or a performance win over
fused native. A partial 14-rule benchmark must not be relabeled as full coverage.
This is indexed Metal matching, not a proof that arbitrary rule joins are GEMM.

59 focused Rust tests passed (joint closure, finite Run, numerical overflow,
union, function update, visibility, combined rulesets and existing hybrid paths),
as did 19 benchmark-script tests. Final gating of the experimental Run path has
additional targeted tests. Compact metrics and input/binary fingerprints are in
[results.json](results.json); reproducible tests are in `tests/joint_sat.rs`,
`tests/scalar_cells.rs` and `cell-assisted-egglog/tests/full_gpu.rs`.

## Layout optimization follow-up

The ordered implementation list is [OPTIMIZATION_PLAN.md](OPTIMIZATION_PLAN.md).
Items 1, 2 and the concrete readback/output-packing parts of 4 and 5 are now
implemented. Item 3 is repaired in the follow-up below; item 6 remains proof work.
No failed whole-Run shortcut was re-enabled.

| Metric | Previous validated run | Compact layout timing run |
|---|---:|---:|
| Inference | 32.658 s | 22.585 s |
| Device execution | 15.171 s | 8.652 s |
| Metal buffers allocated | 7.391 GB | 0.726 GB |
| Returned bindings | 420.862 MB | 65.637 MB |

A subsequent per-ruleset timing/control-cache validation took 24.863 s; it is
also retained, not discarded in favor of the faster number. Exact same-commit
CPU/GPU graphs, Cell-on/off graphs and production Tiger CPU/GPU selected DAGs
passed in both runs. The earlier differences from fused native remain.

The new layout has 69 exact query shapes and 399 shared tuple indexes. Links
are allocated only for the indexes of each event's table. Their size is
O(sum_t E_t I_t), instead of O(E sum_t I_t); hash buckets are also table-sized.
Thread scratch space follows each query's atom/slot dimensions. Output records
contain only their native slots. Queue overflow still triggers exact retries.
No candidate cap, sampling or semantic weakening was introduced.

The added benefit here comes from storage and dispatch layout, not newly baked
Cell coverage. In the timing run, Cell-on/off remained 22.585/24.046 s.
[optimized-results.json](optimized-results.json) includes stage times, the
ruleset diagnostic, fingerprints and all pass/fail outcomes.

Full runs now save exact typed control facts and initial anchors. Subsequent
`--reuse-controls` runs execute only the new GPU candidate and re-run graph
checks; unchanged native/CPU Tiger outputs are reused only after fingerprint
validation. A real full Pass-3 reuse run and small negative cache tests passed.

## Cell query repair

The failed ordinary-Run path has been replaced by a complete, read-only Cell
frontier provider. Default full-rule runs now share one sorted commit for Cell
and GPU bindings. [CELL_QUERY_FIX.md](CELL_QUERY_FIX.md) records the small
order-sensitive counterexample, the real Pass-3 exact checks and the remaining
performance limit. The old failed experiment is historical, not an active flag.

## Post-fix prioritization

The fresh [priority investigation](OPTIMIZATION_PLAN.md) separates actual GPU
matching from maintenance: 7.29 s versus 0.13 s. Index construction is therefore
no longer a high-priority target. An active-anchor routing attempt and exact
whole-query sharing both passed correctness but showed no material device-time
improvement; the former was reverted, and the latter remains opt-in. The next
search target is common partial structure across different rules, not merely
identical complete queries. All individual runs are retained in
[priority-profile.json](priority-profile.json).
