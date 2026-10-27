# Resumable marker context from analysis

This fixes a real loss of information in the Polyspine analysis export. The old
packet had aligned numbers and a `complete` bit. It discarded the addresses of
missing inputs, so GPU-computed child results could not enable their parents.
The new export retains typed input/output coordinates and unresolved ports.

**Scope:** real Raytrace Pass-3 PRE-stage evidence, an offline GPU numerical
projection, and prediction of the original observable extraction-update label.
This is not a replacement egglog runtime or a complete RipSat closure proof.

## Same-data result

The previous source, 12 terms entry snapshots, union-lineage split, 2,309 test
positives, ridge penalty 1, threshold 0.5 and total input width 512 are retained.
No new native Pass-3 execution was needed; the saved lossless journal supplies
PRE-stage evidence. Post-stage rows are used only for validation and labels.

| Representation | Precision | Recall | TP / FP |
| --- | ---: | ---: | ---: |
| Previous affine + guard features | 97.74% | 44.91% | 1,037 / 24 |
| Marker propagation: 1 step | 95.92% | 47.86% | 1,105 / 47 |
| 2 steps | 98.70% | 49.29% | 1,138 / 15 |
| 4 steps | 94.01% | 63.92% | 1,476 / 94 |
| 8 steps | 100% | 66.09% | 1,526 / 0 |
| Numeric fixed point | **100%** | **84.19%** | **1,944 / 0** |
| Fixed-point features shuffled within each snapshot | 94.58% | 43.05% | 994 / 57 |

100% precision is an observation on this finite sample, not a guarantee about
future programs. The 24 new context columns replace the old 24 affine columns;
this is a fixed-width representation comparison, not an additive ablation.

Of the previous false negatives, the new model recovers **all 756** with no
complete input tuple and **156 of the other 516**. It loses five old true
positives, leaving **365** false negatives overall.

Attribution matters: the direct deterministic mask

```
newly available TCPair result OR decreased minimum cost
```

produces exactly the same held-out decisions as the fixed-point ridge model.
Thus the large gain comes from executing preserved dependencies, **not from
ridge learning new arithmetic**. The remaining updates do not change these two
summaries. Full candidate identity and comparison state still matter.

Chronological holdout remains weaker: precision **25.96% → 91.68%**, recall
**41.79% → 44.05%**. The direct deterministic mask there has precision 100% and
recall 43.44%; ridge additionally predicts some updates with false positives.
Later stages predominantly modify existing payloads. Unseen-program
performance has not been tested.

## What analysis now exports

`marker_context.py` exports a finite, typed carrier at a stage boundary:

- One marker record per admitted structural row, retaining its rule recipe,
  output coordinate and input coordinates **even when input values are absent**.
- Complete original binding rows and typed slot dictionaries in a sidecar.
  Different structural alternatives remain distinct records.
- All visible `(Term, cost)` seed alternatives, not uncorrelated marginals.
- The original typed RHS programs, and separately compiled affine coefficients.
- `ExtractedExpr` result links and pending `Smaller` dependencies. A pending
  comparison requires both inputs; one existing branch is insufficient.
- Output-to-producer and input-to-consumer indices. Dependencies preserve
  multiplicity: `Bop(a,a)` adds the cost of `a` twice.
- Initial availability, cost, witness reference and a local update generation.

The GPU numerical executor uses a min-cost projection of these alternatives.
It does **not** carry every generated Term or every equal-cost witness. Full
binding/seed alternatives are retained in the export for richer consumers;
this must not be confused with complete candidate propagation on GPU.

For this source the cost expressions are nonnegative additions/copies and the
helpers select a minimum with a `10^16` initializer. The compiler rejects
nonmonotone expressions and cross-port equality constraints which this scalar
projection cannot represent. The helper interpretation is specific to this
terms interface; it is not a general proof checker for arbitrary helper rules.

## Device execution

The context's row count grows with observed nodes/marker instances. Its record
width is fixed; there is no Cell-instance × every-plan allocation or global
Cartesian product. Eight affine recipes fit a **32-column** arithmetic matrix.

Each propagation step:

```
read child state using exported coordinates
    → gather costs + availability masks
    → shared fixed-coefficient matrix multiplication
    → checked i64 fallback and exact guards
    → min-reduce by output coordinate
    → write next state; feed it into the next step
```

Small integer inputs use FP16 inputs / FP32 matrix accumulation. Other values,
including sentinels, use exact GPU i64 arithmetic. Audit mode checks the
original integer operation order for matrix-safe rows too. Input readiness
is separate from numeric zero. Overflow cannot manufacture an initializer.

The CPU uploads one context and submits device steps. It does not supply new
child costs after each step. A four-byte changed flag controls termination at
checkpoints; there is no intermediate parameter round trip. State generations
track actual projected changes, not elapsed iterations.

1/2/4/8 steps are ablations, not claimed closure. The final experiment continues
to a numeric fixed point: checkpoints range from **8 to 136** across the 12
frames. An unfounded cycle remains unavailable. A finite-carrier invariant
check detects unexpected nontermination instead of accepting truncation as a
saturated result.

The present kernel scans all exported marker records per step. The reverse
dependency index is exported but is not yet an event-driven GPU work queue.
External union, removal or a later ruleset requires a refreshed/versioned
context; reusing stale minima across such changes is not implemented.

## Verification and costs

- GPU state, candidate values and pooled features agree with an independent
  CPU interpreter at every saved checkpoint.
- At the numeric fixed point, **all 12 frames agree with the native post-stage
  oracle on available TCPair results and minimum costs**. This does not compare
  the full Term graph, comparison rows, or extraction quality.
- All ridge GPU scores pass the existing CPU-reference threshold check.
- **39 tests pass**, including real native execution, Metal execution,
  unresolved dependencies, multi-step propagation, alternatives, duplicated
  inputs, pending comparisons, unfounded cycles and integer overflow.
- The existing `scale_capture.py` analysis path now automatically writes
  `marker_context/input.json` and its packets for supported terms programs.
  A native end-to-end fixture verifies this hook before any target saturation.
  Unsupported numerical projections are reported explicitly.

Across these 12 snapshots, context construction took about **8.25 seconds on
CPU**, excluding artifact serialization; GPU propagation totaled about
**0.115 seconds**, and the scoring pass about **0.0057 seconds**. These are
separate diagnostic measurements, not an end-to-end acceleration ratio.
We still need to replace snapshot reconstruction with committed-event updates
before claiming a fast production handoff. No production engine was changed.

## Reproduction and artifact reuse

`MARKER_RESULTS.json` stores counts, correctness checks and provenance.
The detailed wiring, journal and binary packets remain under ignored `out/`.
Use a NumPy-enabled Python (`PY` below). Keep old results instead of repeatedly
running the same native controls.

```sh
PY=/Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python
mkdir -p out/polyspine-marker-build
xcrun swiftc -O experiments/polyspine_ridge/marker_worker.swift -o out/polyspine-marker-build/worker
$PY experiments/polyspine_ridge/marker_study.py export \
  --dataset out/polyspine-scale-data --capture out/polyspine-affine-data \
  --workload out/matrix-dynamic-perf-input/workload.json --output out/polyspine-marker-data
out/polyspine-marker-build/worker out/polyspine-marker-data/input.json \
  out/polyspine-marker-gpu out/polyspine-marker-data/compiled.metal
$PY experiments/polyspine_ridge/marker_study.py audit \
  --exported out/polyspine-marker-data --gpu out/polyspine-marker-gpu \
  --output out/polyspine-marker-audit
$PY experiments/polyspine_ridge/marker_study.py fit \
  --dataset out/polyspine-scale-data --projection out/polyspine-marker-audit \
  --previous out/polyspine-affine-fit --output out/polyspine-marker-fit \
  --worker out/polyspine-residual-build/score-worker
```

The regular Polyspine capture command also emits loadable contexts directly;
its `marker_context/input.json` uses the same worker protocol. Python commands
refuse to overwrite existing output directories. For tests, use the environment
from `AFFINE.md` plus `POLYSPINE_MARKER_WORKER=$PWD/out/polyspine-marker-build/worker`.
