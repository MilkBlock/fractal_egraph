# Native reuse feedback experiment

This exercises the implemented **positive, finite-program subset**. It is not a
claim that arbitrary `.egg` programs are optimized or that native table storage
is virtualized. `reuse-cycle` performs separate optimized, reference, and
trace/ripen-learning passes, so its total time is not a speedup benchmark.

```sh
cargo build --release --offline
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout analyze \
  --source experiments/reuse_feedback/seed.egg --output out/feedback-seed
target/release/egg_layout reuse-bake out/feedback-library.json out/feedback-seed
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout reuse-cycle \
  experiments/reuse_feedback/graft.egg out/feedback-library.json out/feedback-first
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout reuse-cycle \
  experiments/reuse_feedback/graft.egg out/feedback-first/adaptive-library.json out/feedback-repeat
```

All output paths must be new. `seed.egg` bakes the shared A/B fragment. The
consumer in `graft.egg` requires that fragment plus an external Flag fact. The
native LHS changes from three atoms to two through a two-column summary relation.
All later fragment keys must be bound, so summary rows are bounded by the first
table size; a potentially expanding view retains the native plan.
The program also merges two parameter classes; final **all-user-table state and
named-global identity** are compared with an independently executed native program.

Recorded results in `result.json`:

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
