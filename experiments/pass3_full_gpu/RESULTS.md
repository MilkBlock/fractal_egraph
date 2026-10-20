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

The single-Run experiment invoked 518 Cell programs, built 11 local scalar
closures, and reduced device time, but introduced an additional `Smaller`
structure difference relative to the sorted-commit reference. Tiger also found
a `Concat` difference. The exact cause has not been isolated. It is **disabled
by default**, retained only behind `CELL_EXPERIMENTAL_RUN_PROGRAMS=1` for
reproduction. Small tests are insufficient to certify this broader dispatch
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
