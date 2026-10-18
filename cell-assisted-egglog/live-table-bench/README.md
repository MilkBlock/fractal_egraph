# Archived per-rule continuous runtime (7d6d9fc)

The current implementation is documented in [shared-table-bench](../shared-table-bench/README.md). The results and architecture below describe commit `7d6d9fc`; current `--live-worker` runs the shared backend.

This replaces the earlier cold-query measurement as the test of the **incremental
runtime**. It starts from the same actual raytrace Pass-3 checkpoint, executes the
original nested `terms` / `terms-helpers` / `terms-helpers-helpers` schedule, and
submits accepted bindings to the original native RHS. This is not a sequence of
independent snapshots or a replay of a precomputed match list.

Each of the 14 rules gets one persistent worker, shader and set of table indexes.
Subsequent native commits update those tables; completed match receipts remain on
the device. Only new accepted bindings return on the unchecked lane. GPU matching
uses exact tuple indexes and i64 guards, not GEMM. Original Term construction,
i64 RHS computation, `set`, custom `Smaller` merge, union and rebuild stay native.
There is no full original consumer Join on the timed assisted lane.

This measures the generic table-delta mechanism. It uses explicit row fragments,
**not certified multi-atom Cell shortcuts**, and does not claim a whole eggcc
compiler speedup or bake coverage. Tables/indexes are currently separate per rule;
common tables such as TCPair are still replicated across those workers.

## Execution and handoff

1. Prepare sinks and static query wiring once. Subscribe once to the native
   committed-key journal for the union of relevant tables. Bootstrap existing
   rows; no logical history trace or whole-graph export is collected per round.
2. Preserve each original rule's semi-naive watermark. Rows older than its last
   execution populate indexes but do not start matches. Otherwise a live
   checkpoint would accidentally replay already completed work at attachment.
3. Route only committed changed keys to interested rule/atom tables. Updates,
   deletions and subsumption retire immutable row versions; identical-valued
   delete/reinsert events also create new versions, since arbitrary native merge
   effects cannot be memoized by binding value alone; new versions carry
   current canonical typed values. Unions are handled through native rebuild and
   its committed row changes. Literal and within-atom alias checks remain exact.
4. For each original Run boundary, gather all rules' new matches against the same
   frozen native state. Then stage all worklists and execute all original actions
   together. Do not commit one rule while another rule is still matching that
   same boundary. Bindings are sorted before staging for stable submission order.
5. After successful native execution, acknowledge GPU receipts and synchronize
   original native rule cursors. Advancing a cursor is authorized only for this
   complete incremental query provider, not for partial baked Cell coverage.
6. Follow the original Sequence/Repeat/Saturate structure until it finishes.
   Quiet rules do not re-upload data or re-run the GPU join. The native engine
   can subsequently resume without replaying work already completed on the GPU.

The new `apply_completed_cell_ruleset` API is intentionally separate from partial
`apply_cell_bindings`. Partial Cell providers must not claim they exhausted an
original query. The extra bridge APIs only expose/update execution watermarks;
they do not replace Free Join or change ordinary rule execution.

## What is and is not cached

The device retains input rows, tuple indexes, completed bindings and receipts.
No per-round solver or shader compiler is invoked. Growing an index rehashes
existing rows only when bucket capacity changes. Output storage grows without
truncating candidates. Existing records still undergo liveness checks when
sources change; retaining receipts does not imply zero work on every old record.
Old row versions and match records are not garbage-collected yet.

The CPU still projects native committed rows, assigns typed codes, routes them to
rule workers, and reconstructs accepted bindings. There are synchronous per-rule
round trips and acknowledgements. Those costs are measured, not removed from the
reported runtime. No heuristic candidate budget or pattern sampling is used.

## Verification and performance gates

- The same original packet is run natively on a clone of the checkpoint. Both
  paths start with the same original tables, equality state and schedule prefix.
- Final comparison checks all public rows, primitive values, visibility and
  equality modulo newly allocated IDs. Old checkpoint IDs anchor pre-existing
  cycles. Unsupported primitive/container comparison is an error, not skipped.
- Also report every `ExtractedExpr` key's complete TCPair cost set, and test a
  native continuation after handing execution back. These do **not replace**
  the strict same-graph gate; differing intermediate Term/Smaller facts remain
  reported as a failed exact comparison, even if extraction costs agree.
- Strict per-boundary diagnostic mode evaluates the original full native LHS
  independently, filters its result by that boundary's changed-fragment frontier,
  and compares the new GPU matches exactly. Retained active GPU matches must
  also be valid native matches. The oracle supplies no production candidates.
- Diagnostic checks run separately from timing. Timed runs have no full-LHS
  oracle and no graph snapshot inside the interval. Cloning, prefix replay and
  final verification are excluded equally; worker/index setup is reported as an
  additional assisted cost. Setup plus run is reported as well as steady run.
- Native search+apply includes RHS staging. Native insert/merge and rebuild have
  their original timers. GPU device timestamps are a subset of roundtrip time;
  they cannot be added to it or called end-to-end speedup.
- One execution per result, no averages. Metal/OS caches are not flushed. This is
  the current native fork, not the immutable pristine upstream control.

The early bring-up runs exposed an initial watermark loss and a missing handback
cursor update; both have regressions. Cold-query timings from the previous task
remain separate. The final report explicitly records whether exact graph equality
passed. A benchmark that finishes is not automatically an accepted speedup.

## Reproduce

Build uses live worker protocol **7**; offline matrix ABI is still 5.

```sh
mkdir -p out/live-table-runtime
xcrun swiftc -O cell-assisted-egglog/matrix.swift \
  -o out/live-table-runtime/matrix-metal-v7

python3 cell-assisted-egglog/bench_snapshot.py \
  --output out/raytrace-live-new \
  --live-worker out/live-table-runtime/matrix-metal-v7
```

The helper builds the release REPL; `--skip-build` reuses a known current build.
Its default captured pass and provenance checks are shared with
[the previous real checkpoint harness](../snapshot-bench/README.md). No new
production frontend, trace collection or bake is needed.

For a separate correctness replay, add `--live-check` and a different output
directory. Its time includes native oracle queries and must not be compared with
an unchecked native execution. Inside the REPL:

```text
:until /3/repeat0/4
:bench-live measured /absolute/path/to/matrix-metal-v7
:quit
```

`check` as a third argument enables the per-boundary diagnostic oracle.

## Code and tests

- `src/table_live.rs`: persistent rule sessions, row deltas, frozen matching
  boundaries, original schedule traversal and handoff.
- `src/table_query.rs::plan`: shared static table-query compiler.
- `tables.metal` / `matrix.swift`: delta ownership, seeded old rows, pending-only
  output, retained indexes and receipts. Normal numeric reduction is unchanged.
- `egglog/src/cell_assist.rs`: batch original RHS execution and complete-provider
  cursor updates; partial-provider API keeps its previous scope.
- `egglog-bridge/src/rule.rs`: read-only initial watermark and trusted handback.
- `examples/support/live_table_bench.rs`: full packet controls, exact graph and
  extraction-cost checks.
- `tests/table_live.rs`: recursion, custom merge/self-joins, late data, visibility,
  union, no idle replay, initial handoff and native handback.

```sh
CELL_MATRIX_WORKER="$PWD/out/live-table-runtime/matrix-metal-v7" \
  cargo test --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted --test table_live
```

Recorded [RESULTS.md](RESULTS.md) preserves both failed exact-graph comparisons,
including the projection that excludes Smaller. Matching node counts and equal
extraction costs are not called graph equality. The final timed and checked runs
use identical runtime/source/worker hashes. Raw outputs are
`out/raytrace-live-version-bench` and `out/raytrace-live-version-checked`.
