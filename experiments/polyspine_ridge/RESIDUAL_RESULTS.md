# Residual Polyspine: pre-stage state matters

On the SAME 47 newly enabled purity answers and 109,380 negatives in Raytrace Pass 3, **one step of rule-derived GPU propagation recovers 45/47 with zero false positives**. Ridge on the projected context also recovers 45/47, but adds false positives. The gain belongs primarily to the representation and deterministic propagation, not to learning.

## Why this differs from the first experiment

The old masking policy erased valid entry-state operator/child purity along with future target labels. The new protocol retains committed entry facts but trains/scores only roots lacking their own target answer. All features are frozen before the stage. The 10,121 native Run update flags and the old per-root bags/order/entry visibility were checked before reusing old labels. An independent native replay additionally rechecked entry and exit labels at every persisted typed root; all 12 stages agree.

The shape-only model is retrained on this pending-only objective. Do not attribute the difference between it and the old all-state model solely to feature changes.

## Matched explicit-coordinate models

| Model | Recovered positives | False positives | Precision | Recall |
|---|---:|---:|---:|---:|
| shape_pending | 14/47 | 8 | 63.64% | 29.79% |
| state_1 | 45/47 | 5 | 90.00% | 95.74% |
| state_2 | 45/47 | 4 | 91.84% | 95.74% |
| state_4 | 45/47 | 6 | 88.24% | 95.74% |
| state_8 | 45/47 | 6 | 88.24% | 95.74% |
| erased_8 | 38/47 | 4 | 90.48% | 80.85% |
| shuffled_8 | 14/47 | 8 | 63.64% | 29.79% |
| direct propagation, 1 layer(s), no ridge | 45/47 | 0 | 100.00% | 95.74% |
| direct propagation, 2 layer(s), no ridge | 45/47 | 0 | 100.00% | 95.74% |
| direct propagation, 4 layer(s), no ridge | 45/47 | 0 | 100.00% | 95.74% |
| direct propagation, 8 layer(s), no ridge | 45/47 | 0 | 100.00% | 95.74% |

The 1/2/4/8 depths are predeclared ablations, not a chosen optimum. In this held-out population, all reachable new positives are already one-hop consequences of entry-state facts. More depth does not help. Empty/unknown results are not proofs of impossibility.

The erased-prestate control removes previously committed recursively produced predicates while retaining static operator facts. The shuffled control assigns residual feature rows to the wrong roots. Both lose useful information; all such controls remain speculative and never commit effects.

## Retained hash-bag instability

| Layers | State mixed into hash bag: TP / FP | Explicit numeric state: TP / FP |
|---|---:|---:|
| 1 | 43 / 3 | 45 / 5 |
| 2 | 44 / 3 | 45 / 4 |
| 4 | 14 / 5 | 45 / 6 |
| 8 | 45 / 1 | 45 / 6 |

The explicit layout reserves 32 of 512 columns for numeric readiness/state and leaves the other 480 unchanged across depth variants. This avoids rehashing and renormalizing earlier state when depth grows. It removes the observed four-layer recall collapse, but does **not** uniformly reduce false positives: the retained eight-layer hash-bag result has fewer false positives than the explicit model.

## The two misses are structural transitions and union

- `Value(768223)` becomes canonical `Value(8226)`; destination already pure at entry: **True**; newly visible constructor alternatives: AddContext, Concat, SubTuple.
- `Value(768225)` becomes canonical `Value(8819)`; destination already pure at entry: **True**; newly visible constructor alternatives: AddContext, Concat, SubTuple.

At entry each source root has only `TupleRemoveAt(tuple, 0)`, and tuple-length is already 4. The native rules include:

```lisp
(rewrite (TupleRemoveAt tuple idx)
  (Concat (SubTuple tuple 0 idx)
          (SubTuple tuple (+ idx 1) (- len (+ idx 1))))
  :when ((= len (tuple-length tuple))) :ruleset always-run)
```

The before/after observations establish newly exposed alternatives and union to previously pure classes. They are not a complete per-rule causal proof of the intervening derivation. This evidence was collected after model fitting and was not added to its training or features.

A frozen purity dependency graph cannot discover a future structural connection just by iterating more times. These cases motivate representing conditional structural/equality transitions next, rather than claiming all dynamic egraph behavior is solved by this projection.

## What runs where

The compiler admits **14/16** unary-producing rules. The rejected guarded Get/TypeList and multi-anchor Call paths remain listed. CPU preparation copies raw typed tables, assigns coordinates and compiles per-rule column selectors; it does not join those rows into per-instance bindings for GPU input. GPU evaluates the shared selectors and preserves whole-alternative conjunctions. A separate CPU expansion is only the differential oracle.

The admitted support graph averages **1.507 predicate dependencies per rule row**. That measures this rule family, not the sparsity of arbitrary egraphs or all Pass-3 joins.

All 12 recorded entry states and both state-retention controls pass GPU/CPU projection checks. Ridge matrix scoring has **0 threshold differences** and max score error **3.5762786865234375e-07**. The old SPINE003 packet also remains correct.

Prestate GPU projection (12 frames, eight layers each): **3.645 ms**. Ridge scoring of all seven ablations: **1.744 ms**. Projection uses sparse gathers and reductions, not Tensor Core GEMM. Only scoring uses matrix multiplication.

Offline snapshot observation: **35.820 s**; raw-context preparation and previous-feature verification: **23.917 s**; independent CPU projection diagnostic: **1.664 s**; fitting-feature packing/checks: **95.390 s**. GPU features are read back for offline fitting. This is not resident end-to-end inference and provides no eggcc speedup claim.

Separate borrowed-label audit: **78.526 s**, diagnostic only.

Nineteen regressions include complete versus partial alternatives, seeded/unseeded cycles, repeated variables, subsumed seeds, unsupported effects, actual GPU projections, numeric-column routing and legacy packet compatibility.

## Provenance

- Source: `c727cdf724b020ae585c4051fda36b66576c1ce4252a4d9d59fbb7522d0c7d4f`
- Capture: `287d5217c74fe3d5ba2dff249b14f196b0979f19e919f43c95c33c3611c2abd4`
- Score worker: `b6bfc1df97e69086d0f81ec0e9efa4eb5d2d19d071f053defe3167d9a02f1cb5`
- Full saved results: `out/polyspine-residual-explicit` and `out/polyspine-residual-tokens`.
- Native case evidence: `out/polyspine-residual-cases/cases.json`.
- The earlier report-emission failure is retained in `out/polyspine-residual-fit/failure.json`; it is not an accepted result.
