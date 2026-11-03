# Pinned marker-shape experiment: sum-bits Pass 3

This tests source-level auxiliary-view layout on the **standalone CPU Cell
engine**. It is not pristine egglog, GPU, or a new RipSat closure/sat-skip result.
The general marker generator and solver are unchanged. Multi-rule compositions
are disabled in every lane; existing multistage point queries and copied marker
fields are kept, with the same storage/takeover settings.

## What was compared

All variants start from the same verified `source_sk_mk.egg` pipeline. SK and
Bake receipts are reused. A pinned experimental marker pass emits another legal
`.egg`, then uses the ordinary typed frontend, Marker IR validator, Recipe IR
and Cell CPU target. No raw `source_core` execution bypass is used.

- **baseline**: identical marker definitions, with the same AST roundtrip as
  the candidates. This avoids comparing different parser/control normalization.
- **remove**: remove `CellSpineView13` and its publisher actions; expand its
  two Get facts back into consumers and any dependent marker completion rules.
  Other marker guarantees remain unchanged and independently validate.
- **if**: additionally use `ExperimentIfGetPair` for compatible If consumers.
  Its completion joins an actual If row with the two branch Gets. Other
  consumers retain their expanded original Get queries. Both orientations of
  the old pair are mapped to the corresponding then/else outputs.

`If` is subsumable in this source. Therefore the experimental view is an ordinary
Unit function, **not** an immutable Cell certificate. Consumers retain their
actual If atom to check current visibility and the complete parent binding.
The auxiliary tuple contains `(u,then,i,v,else)`; multiple parent If rows remain
distinguished by the original consumer atom. No false permanent If guarantee
was introduced into the normal Marker IR.

The If variant rewrites 12 compatible consumer uses; eight other consumers and
one dependent prefix completion expand the Get pair. Removal eliminates 23
original publisher actions for the old view.
The If view uses exact scheduled completion, not a new direct publisher.

## Single-run measurements

| Metric | Baseline | Remove pair marker | If-context pair |
|---|---:|---:|---:|
| Inference | 0.520016 s | **0.217336 s** | 0.224555 s |
| Speedup over baseline | 1.00x | **2.39x** | 2.32x |
| Peak RSS | 230.45 MiB | 206.89 MiB | 205.36 MiB |
| Candidate row visits | 686,858 | 151,874 | 149,157 |
| Key lookups | 213,772 | 97,113 | 93,925 |
| Repair dependency visits | 38,283 | 13,384 | 13,441 |
| Union requests | 4,627 | 4,627 | 4,627 |
| Effective unions | 1,089 | 1,089 | 1,089 |
| Function merge calls | 3,473 | 3,473 | 3,538 |
| All auxiliary rows | 6,820 | 1,152 | 1,161 |
| Old pair / new If pair rows | 5,668 / 0 | 0 / 0 | 0 / 9 |

The historical 0.525 s baseline was not reused: the shared-key decoder changed
the actual CPU binary fingerprint. The replacement baseline was measured once;
build, compilation and audits were outside inference timing. No averages or
repeated selection of the fastest run were used.

Search falls from 259.44 to 113.46 ms on removal; repair from 97.51 to 33.25 ms;
RHS from 73.01 to 36.12 ms. The cost was not confined to publishing the marker:
consumer searches, dependency repair and indexes also paid for its large domain.

The old view has exactly `sum_i GetCount(i)^2` rows:
`62² + 27² + 23² + 19² + 14² + 3² = 5,668`, from only 148 Get rows.
This is not evidence of 5,668 useful Cells or high closure coverage.

**The If view is 3.32% slower than simply removing the pair marker in this run.**
It saves a further 2,717 candidate visits but adds maintenance and 65 merge calls.
Thus the demonstrated gain is removing a negative-value view. This input does
not show an additional net benefit from the contextual replacement.

## Correctness and limits

Each candidate passed **1,000** exact projected snapshots: initialization plus
every non-marker-completion Run, including structural-key stages. Ruleset order
and iteration coordinates agree, and the compiled control trees are identical.
The projection excludes only explicitly declared auxiliary marker/receipt tables
and the experimental Unit view. All ordinary table names and schemas must agree.

Every endpoint has the same 3,519 original records and 2,324 mapped equality
classes, with exact values, equivalence and visibility. Both final subsumed If
rows remain subsumed. This is the existing anchored exact comparator, not a
hash, node-count-only check, or an extraction-quality surrogate. Logical helper
match counts change, as expected; no claim of identical helper binding streams
is made. Tiger extraction was not separately rerun.

Four targeted transformer tests check branch orientation, full parent-guard
retention, unrelated consumers, dependent prefix expansion and rejection of a
different marker layout. The current transformer reproduces all measured `.egg`
sources byte for byte. Timing uses the existing `suite.timed`, work summaries use
the existing composition benchmark metrics, and graph checks use the existing
CPU/native comparator. Join-depth histograms are unavailable on this CPU path;
total candidate visits are not presented as a depth distribution.

This is one small Pass 3 input, not a cross-workload rollout or a stable claim
about the small 3.32% difference. Nothing is enabled by default or merged into
the main workspace.

Compact measurements/fingerprints: [results](marker-shape-results/sum-bits.json).
Generated sources, complete receipts and raw measurements are under
`out/marker-shape-sum-bits/`; generated artifacts and binaries are not tracked.

## Reproduce

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml --bin eggfuse-cell-runtime --example project_stages
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.marker_shape --pipeline out/compositions-sum-bits-01/compile --output out/shape-recheck/baseline-compile --mode baseline
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.marker_shape --pipeline out/compositions-sum-bits-01/compile --output out/shape-recheck/remove-compile --mode remove
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.marker_shape --pipeline out/compositions-sum-bits-01/compile --output out/shape-recheck/if-compile --mode if
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.marker_shape_bench --root out/shape-recheck --output out/shape-recheck/results
/opt/homebrew/bin/python3 -B -m unittest experiments.cell_runtime.test_marker_shape -v
```

Use new output directories. The shape pass is intentionally pinned to this
Get/Get layout and If example; it is not the proposed generic implementation.

## Generic method to review before implementation

1. Offer **no materialization**, the existing view, and consumer-context-filtered
   views as alternatives. Do not assume adding a marker is always beneficial.
2. Find context in the original typed consumer: an existing atom or Spine that
   constrains the fragment's otherwise free input coordinates. Preserve full
   correlated bindings, including multiple constructor alternatives per eclass;
   do not assume a root uniquely determines its operands.
3. Separate **stable facts being replaced** from **context restricting the
   domain**. Mutable/subsumable context stays in the consumer as a live check.
   Exact completion still covers initial, late and union-created bindings.
4. Reuse the existing solver's measured-cost mode. Charge generation, index and
   repair maintenance plus consumer search; share prefix cost once. The current
   structural atom-count objective misses the Cartesian domain expansion.
5. Profile/validate on the small Pass 3 suite and keep the original-query option.
   Profiling selects an execution representation; it never justifies dropping
   legal matches. Accept using net inference time and memory, not marker counts.

This method is proposed only. General candidate generation, conditional Marker
IR and the solver have not been changed by the experiment.
