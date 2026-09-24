# Native reuse feedback experiment

This exercises the implemented **positive, finite-program subset**. It is not a
claim that arbitrary `.egg` programs are optimized or that parameterized native
facts are virtualized. `reuse-cycle` now combines planned execution and source
trace/ripen learning in one source capture. It performs no independent reference
rerun; `reuse-run` and regression tests provide that validation separately.

```sh
cargo build --release --offline
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout analyze \
  --source experiments/reuse_feedback/seed.egg --output out/feedback-seed
target/release/egg_layout reuse-bake out/feedback-library.json out/feedback-seed
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout reuse-cycle \
  experiments/reuse_feedback/graft.egg out/feedback-library.json out/feedback-first
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout reuse-cycle \
  experiments/reuse_feedback/graft.egg out/feedback-first/reuse-library.json out/feedback-repeat
```

All output paths must be new. `seed.egg` bakes the shared A/B fragment. The
consumer in `graft.egg` requires that fragment plus an external Flag fact. The
native LHS changes from three atoms to two through a two-column summary relation.
All later fragment keys must be bound, so summary rows are bounded by the first
table size; a potentially expanding view retains the native plan.
The program also merges two parameter classes; final **all-user-table state and
named-global identity** are compared by `reuse-run` with an independently executed
native program; the single-pass command does not make that extra execution.

Historical three-pass results in `result.json` (kept as the original evidence):

| Metric | First cycle | Repeated cycle |
|---|---:|---:|
| Native summary-row reuses | 2 | 2 |
| Unexpected committed graft episodes | 1 | 0 |
| Newly admitted enclosure descriptors | 3 | 0 |

Novelty means a previously unseen typed rule/relative-wiring signature. Once the
first graft is observed it predicts later instances. It is **not** a proof that a
new composition is saturated or that ripen can be skipped. A library only admits
new bodies from independently completed native ripen sessions. Incomplete row
witnesses are reported rather than guessed. Graft evidence is aggregated by
signature; raw native event batches are drained after each step.

The tiny example is slower under view planning because extra compilation/refresh
cost dominates. Therefore the optimizer remains opt-in. Existing `cargo bench
--bench reuse_graph` guards API regressions, not end-to-end optimizer speed.

`reuse-run SOURCE LIBRARY` executes the restricted optimized program and an exact
reference check. `--learn-grafts OUTPUT.json` additionally records native graft
witnesses and saves learned prediction signatures. The ordinary `analyze` path
continues to use the original native matcher and full source provenance.

## Single-pass continuation (2026-09-24)

`reuse-cycle.json` records `source_executions: 1` and points to `reuse-library.json`.
Local ripen jobs and summary refreshes are additional work, so this count alone is
not a latency claim. `analyze --reuse-library FILE --reuse-plan --save-history`
uses the same implementation. Plain `analyze --reuse-library FILE` retains original
matching. Subsumption/update programs fall back to native execution; residual
nested-call keys whose compiled names may disappear also retain native rules.

```sh
cargo test --offline --test reuse_singlepass --test native_fact_query \
  --test reuse_graph --test reuse_planner --test reentry_history_regression
cargo test --offline --test native_shared_snapshot
cargo test --offline --manifest-path egglog/core-relations/Cargo.toml --lib
cargo bench --offline --bench native_shared_snapshot
```

The single-pass test compares Coarse/Smooth definitions/layers and learned library
counts with unplanned captures. It includes three cycles, replacing `(V 1)/(V 2)`
with `(V 11)/(V 12)` and changing source paths. The second and third cycles add no
bodies, descriptors or source entries. Runtime admission retains the first validated
origin for each (descriptor, replay source); per-run origin evidence remains in
ripen output instead of indefinitely growing the reusable library.

Source witnesses are expanded at match time to the real A/B/Flag rows and producer
versions. A summary-exposed result variable such as `root` must not become a new
external ripen parameter. Missing row keys mark witnesses incomplete, not invented
producer links. Tests include nested residual fallback and subsume fallback, plus
DABCABC and branching cells that must remain `ObservedUnfinished`.

`singlepass-results.json` records this continuation's measurements. Native snapshot
sharing saves duplicate **row payloads** at the same binding; indexes and metadata
still clone. This is not cross-parameter virtual fact storage or proof of an
end-to-end speedup. The historical native math timing comparison is only a smoke
check against the previous session's saved pre-COW sample.

## Typed filters and native scan evidence

`planner-scan-results.json` measures **actual native** table row visits, with one
shared A/B view, 64 bindings and 1/8/16 consumers. Including refresh, scans change
by +33.3% / -18.5% / -25.5%. Final states are independently checked. The losing
one-consumer case remains a regression control; these counts are not elapsed-time
speedups, mutation counts or a reason to enable planning globally. `$uf` table
profiling is explicitly unsupported; user and summary table scans are covered.

```sh
cargo build --release --offline --features native-read-profile
python3 tools/bench/measure_reuse_scans.py --output /tmp/reuse-scans.json
cargo test --offline --features native-read-profile \
  --test native_read_profile --test reuse_scan_profile --test reuse_features
```

The feature filter is used before exact library-prefix matching. It adds typed
columns, canonical literal positions and equality within the first four columns.
Unknown boundary/condition/staleness cases remain Maybe. `filter-results.json`
records synthetic maintenance/probe microbenchmarks and the fixed bitmap's
saturation under a large distinct-operator workload. This is kept separate from
the native evidence. Each immutable filter uses 160 bytes; it is rebuilt from
validated library bodies, never patched to represent a changed runtime instance.
