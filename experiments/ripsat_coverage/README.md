# Frozen RipSat coverage: fact → loopfact

**The present library has substantial expression-shape coverage, but the measured
prefix coverage does not yet establish a large Join-elimination opportunity.**

This diagnostic runs the actual local native egglog engine on real eggcc exports.
It does not change source rules, the planner, or the runtime. It trains on `fact`
and freezes before evaluating `loopfact`. No held-out pattern or signature is
added to the library.

## Remeasurement after analyze/snapshot integration (2026-10-04)

Rebuilt the diagnostic on the current local runtime and repeated `fact` training
followed by frozen `loopfact` evaluation. The requested effective-apply signature
metric remains **639 / 2,890 = 22.1107%**. All five other headline metrics in the
original table are unchanged when retaining the old library.

To distinguish execution changes from library changes, three training-only inputs
were measured:

| Pattern input | Pattern entries | Trained signatures | Held-out known effective applies |
| --- | ---: | ---: | ---: |
| Original empirical library | 43 | 1,678 | 639 / 2,890 = 22.11% |
| Latest completed observations only | 120 | 5,724 | 585 / 2,890 = 20.24% |
| Original plus latest observations | 163 | 7,402 | 639 / 2,890 = 22.11% |

The new input comes from the 121 completed observational states of the full native
analyze replay; one background-only state is excluded. These are occurrence-level
patterns and include duplicates, not 120 distinct learned families. Marker rows
are converted into boundary-slot incidences before the existing pattern importer
runs; literal payloads, aliases and visibility are preserved. No unfinished or
rejected cell is admitted. The full-replay observations use the local ripen
schedule, whereas the older probe used the original optimizer schedule; they
must not silently replace one another in an attribution experiment.

The new-only input covers 325/557 Expr rows, 79/157 Expr classes and 491/2,674
constructor rows. Unioning it with the old input restores 340/557, 87/157 and
729/2,674, but adds **zero** new covered rows or effective-apply hits on this
held-out workload. Every original trained signature remains in the merged set.
The merged library also leaves pre-step full-LHS coverage at 381/6,788 (5.61%)
and multiple-internal-read coverage at 159/6,788 (2.34%).

All three runs preserve the native traced control's final state, and each test
leaves its library hash/signature count unchanged. Conditional on an existing
cell anchor, the old/merged hit rate is 639/957 = 66.77%; this is **not** the
all-effective-apply denominator requested here. Effective applies count inserted/
updated writes and successful union displacement; subsume-only effects are excluded.

The merged diagnostic takes about 3.67 s on held-out evaluation versus 1.05 s for
the old input in these single runs, with more duplicate embeddings/signatures.
These are audit costs, not runtime Join improvements. Parent-snapshot reuse changes
initialization work in `analyze`; this diagnostic still measures frozen positive
pattern support and observed effect signatures, not certified transition reuse.
See `remeasurement.json` for hashes, counters, control checks and exact results.

Reproduce the new-only and merged variants after the ordinary control run below:

```sh
python3 experiments/ripsat_coverage/refresh.py \
  --analysis out/eggcc-full-replay-20261004-stack16m/run \
  --output out/coverage-new-patterns
python3 experiments/ripsat_coverage/run.py \
  --train-source /tmp/eggcc-set-final/live/source.egg \
  --test-source /tmp/eggcc-marker-loopfact.egg \
  --patterns out/coverage-new-patterns --output out/coverage-new
python3 experiments/ripsat_coverage/refresh.py \
  --merge-old experiments/ripsat_coverage/patterns.json \
  --merge-new out/coverage-new/train/library.json \
  --output out/coverage-union-patterns.json
python3 experiments/ripsat_coverage/run.py \
  --train-source /tmp/eggcc-set-final/live/source.egg \
  --test-source /tmp/eggcc-marker-loopfact.egg \
  --patterns out/coverage-union-patterns.json --output out/coverage-union
```

## What is frozen

The earlier probe produced 44 empirical finite closure snapshots. One contains
only global background facts, so 43 patterns remain. `patterns.json` stores these
patterns compactly. They preserve literal payloads, sorts, aliases, and row
visibility; synthetic RipenInput marker rows are removed. The underlying
interfaces are still symbolic, and positive containment permits variable aliasing
and extra surrounding facts. Thus matches are **optimistic structural embeddings**,
not certificates that a runtime instance is closed under all outside contexts.

Training learns 1,678 strict observed read/effect signatures. A signature includes
source pattern, rule identity, stable LHS positions, cell-relative value slots,
external/residual aliases, literals, writes with before/proposed/after state,
and union endpoints. It does not contain a certified destination-state adapter.
Signature hits are **not** reported as actual executable state-machine reuse.
A frozen self-check on `fact` recognizes all 797 anchored effective applications.

## Results

| Metric | fact: training | loopfact: held out |
|---|---:|---:|
| Expr e-nodes covered | 329 / 419 = 78.52% | **340 / 557 = 61.04%** |
| Expr e-classes covered | 99 / 134 = 73.88% | **87 / 157 = 55.41%** |
| All stored constructor rows covered | 626 / 1,914 = 32.71% | **729 / 2,674 = 27.26%** |
| Logical matches | 4,664 | 6,788 |
| Complete LHS in one pre-existing cell | 432 = 9.26% | **381 = 5.61%** |
| At least two LHS reads in one cell | 271 = 5.81% | **159 = 2.34%** |
| Successful LHS read atoms covered | 2,833 / 10,500 = 26.98% | **3,506 / 15,185 = 23.09%** |
| Effective insert/update/union applications | 2,060 | 2,890 |
| Effective applications anchored to a cell | 797 | 957 |
| Frozen read/effect signature hit | not scored while learning | **639 / 2,890 = 22.11%** |

Of the held-out anchored effective applications, 639/957 (66.77%) have a known
signature. That conditional hit rate must not be presented as overall coverage.
Only 24 of the 43 frozen patterns match the final held-out graph, yielding 320
positive embeddings. Overlapping embeddings count each row/e-class only once.
The constructor denominator includes lowered relation carrier constructors as
well as expression constructors; it is not synonymous with Expr e-nodes.

The 381 fully contained held-out logical matches have no recorded insert/update/
union effect. Mature closed interiors are expected to produce confirmations, so
looking only at changing applications would miss this potential reuse. However,
most changing applications still cannot be fully explained by a pre-existing cell.

## Why the three percentages mean different things

Final structural coverage says that many facts fit known shapes after execution.
Temporal LHS coverage asks whether the shape already existed **before the matching
rule batch**. This prevents future RHS facts from explaining earlier matches.
Signature coverage additionally requires a training-observed binding/effect layout.

These statistics deliberately measure fully materialized closed-pattern support.
They do **not** measure recognition from a smaller trigger followed by virtual
closure completion. That requires the missing trigger-to-state and target-state
adapters. Consequently this report does not rule out the user's trace-driven
state-machine design; it identifies what the current frozen representation can
and cannot substantiate.

For a simple rewrite replacing k covered LHS reads by one marker, the held-out
successful-read contraction count is 176/15,185 = 1.16%. This is **not a bound on
all possible speedups**: it excludes failed Join candidates, differences in
selectivity, and suppressing entire already-completed internal rules. Actual Join
savings remain unmeasured. Native search-and-apply timings and row counters are
reported for diagnosis, not multiplied by coverage percentages.

## Current infrastructure limit

`certified_state_transition_coverage` is null, not 22.11%. The frozen library here
contains empirical snapshots and effect signatures. It has no certified eggcc
source-state → target-state adapters. The production pipeline can now save completed observations, but these eggcc
states still lack certified transition adapters; Math-specific execution support is not silently treated
as general eggcc support. This experiment therefore does not claim that the
runtime can already emit all covered abstractions without searching.

## Measurement safeguards

- Before each native ruleset step, read current rows into a temporary independent
  Rust index. Pattern joins run on those copies, not the live engine. They do not
  warm native Join indexes or build runtime markers.
- Drain actual native trace receipts after the step. Compare their source reads
  only with the saved pre-step coverage. Keep logical matches separate from
  committed insert/update/union effects; subsume-only effects are not in the latter
  denominator.
- Preserve the original schedule's repeat/saturate/sequence and stopping behavior.
  `until` is rejected by this diagnostic walker rather than approximated.
- Check the final state against ordinary native command-stream execution with
  the same dependency tracing. Both workloads pass. This does not claim that
  tracing preserves untraced order-sensitive `:merge new` choices.
- No candidate or embedding cutoff. Background `TNil`, `PureTypeList`, and
  `TypeList-length` facts are required by patterns where applicable but receive
  no coverage credit.
- The library hash and signature count remain unchanged during held-out evaluation.

The audit adds roughly a second of diagnostic work per small workload and is not
proposed as the fast runtime. No giant raw trace stream is saved. The learned
signature file is generated outside Git; only pattern input, code, and compact
results are tracked.

## Reproduce offline

```sh
cargo build --release --offline --features native-read-profile --example ripsat_coverage
cargo test --offline --example ripsat_coverage
python3 experiments/ripsat_coverage/run.py \
  --train-source /tmp/eggcc-set-final/live/source.egg \
  --test-source /tmp/eggcc-marker-loopfact.egg \
  --output out/ripsat-coverage-new
```

Both inputs are actual one-pass exports generated by the existing eggcc binary
with `--run-mode egglog --stop-after-n-passes 1`; source hashes are in `results.json`.
The native engine is the local modified runtime, not the pristine baseline.
Tests cover overlapping patterns, literal constraints, hidden rows, and rejecting
future-state coverage of earlier matches.
