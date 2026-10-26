# Polyspine residual context: preserve what is already known

The first experiment masked all current-stage analysis, including facts already
committed **before** the stage. This was stricter than necessary for causal
prediction. In particular, erasing operator purity and child purity discarded
the state that a later Spine should reuse. The relevant leakage boundary is
time, not merely the name of the predicted predicate.

This follow-up keeps pre-stage facts, predicts only roots whose answer is still
absent, and projects the connected residual requirements on the GPU. Native
post-stage truth remains withheld until scoring. No production rule or schedule
is changed.

## Representation

The typed native rules are inspected once. Unary existence predicates reachable
from ExprIsPure form a family; this also discovers ListExprIsPure and the three
operator-purity predicates. Fourteen of sixteen producing rules admit one
structural constructor row, unary premises and re-emission of that same anchor.
The multi-anchor Call rule and guarded Get/TypeList rule are explicitly rejected.

For an accepted rule, the compiler records column selectors, for example:

```
native Bop row: [operator, left, right, result]

requirements: BinaryOpIsPure(column 0)
              ExprIsPure(column 1)
              ExprIsPure(column 2)
output:       ExprIsPure(column 3)
```

CPU preparation copies raw typed rows, assigns coordinates and supplies existing
predicate bits. It does **not** enumerate per-instance rule bindings or join
structural atoms. Each shared selector program is applied to its raw rows by the
GPU. A separate CPU incidence expansion is used only as a differential oracle;
it is not uploaded as the GPU plan and its time is reported.

Each layer performs:

1. Gather the required coordinates from the current state matrix.
2. Require **all premises of the same row**; partial premises from two different
   alternatives cannot be combined.
3. Take OR across complete alternatives that produce the same predicate key.
4. Retain previous truths and record both reached truth and the best fraction of
   ready premise occurrences (including repeated ones). Repeat at fixed depths
   1, 2, 4 and 8 for ablation. The separate support-graph arity metric counts
   distinct dependencies, not repeated premise occurrences.

This is a sparse gather/reduction with AND/OR, **not** GEMM, a learned operator or
a full implementation of dynamic union/rebuild. The raw matrix has a finite
size for each recorded entry state. Structural growth during the stage is not
silently assumed to have been represented.

## Ridge and attribution

All models now train on pending roots only, with the same chronological split
(cycles 0–5 train, 6–11 test), regularization 1 and threshold 0.5. This objective
change has its own shape-only control; comparison with the previous 19/47
result is not a feature-only ablation.

Two coordinate layouts are retained:

- **tokens**: append projected state as categorical entries in the old hash bag.
  This exhibited a severe four-layer regression; the result is retained.
- **explicit**: reserve 32 of the 512 columns for numeric residual state. Keep
  the other 480 structural coordinates identical across all depth variants.
  Adding a layer no longer renormalizes or hashes the earlier numeric state.

The SPINE004 extension of the existing matrix worker supports those explicit
columns. FP16 storage, FP32 GEMM scoring and the old packet modes are preserved.
Known-state erasure and shuffled-root controls isolate whether the state and
its connection to the queried root matter. Raw direct propagation is reported
alongside every learned model: its benefit must not be credited to ridge.

The GPU projection currently writes its features for offline fitting/packing,
then the score worker reads them. This is **not a fused resident runtime**.
Snapshot collection, CPU preparation, model fitting, GPU projection and GPU
scoring have separate costs. None establishes end-to-end eggcc acceleration.

## Reproduce without repeating unchanged controls

```sh
# Needed once: the old dataset did not retain the missing entry facts/topology.
python experiments/polyspine_ridge/residual_capture.py \
  --dataset out/polyspine-pass3-data \
  --frontend out/matrix-dynamic-bench-build/frontend \
  --output out/polyspine-residual-data

mkdir -p out/polyspine-residual-build
xcrun swiftc -O experiments/polyspine_ridge/residual_worker.swift \
  -o out/polyspine-residual-build/worker
xcrun swiftc -O experiments/superposed_key_memory/gpu.swift \
  -o out/polyspine-residual-build/score-worker

out/polyspine-residual-build/worker \
  out/polyspine-residual-data/capture.json out/polyspine-residual-projection \
  experiments/polyspine_ridge/residual.metal

python experiments/polyspine_ridge/residual_fit.py \
  --dataset out/polyspine-residual-data --projection out/polyspine-residual-projection \
  --worker out/polyspine-residual-build/score-worker --coordinates explicit \
  --output out/polyspine-residual-explicit
# --coordinates tokens reproduces the retained hash-bag ablation.

POLYSPINE_RESIDUAL_WORKER="$PWD/out/polyspine-residual-build/worker" \
POLYSPINE_SCORE_WORKER="$PWD/out/polyspine-residual-build/score-worker" \
python -m unittest discover -s experiments/polyspine_ridge -p 'test_*.py'

# Post-hoc native diagnosis only; do not feed these observations into training.
python experiments/polyspine_ridge/residual_cases.py \
  --capture out/polyspine-residual-data --frontend out/matrix-dynamic-bench-build/frontend \
  --output out/polyspine-residual-cases

# Older captures reused labels after feature/order checks only. This independent
# audit rechecks all entry/exit labels; new captures also check exit labels inline.
python experiments/polyspine_ridge/residual_label_audit.py \
  --capture out/polyspine-residual-data --frontend out/matrix-dynamic-bench-build/frontend \
  --output out/polyspine-residual-label-audit

# Render saved measurements without rerunning inference or fitting.
python experiments/polyspine_ridge/residual_report.py \
  --explicit out/polyspine-residual-explicit/results.json \
  --tokens out/polyspine-residual-tokens/results.json \
  --cases out/polyspine-residual-cases/cases.json \
  --label-audit out/polyspine-residual-label-audit/audit.json \
  --output experiments/polyspine_ridge/RESIDUAL_RESULTS.md
```

The final report is [RESIDUAL_RESULTS.md](RESIDUAL_RESULTS.md). The remaining
misses can be inspected natively with `residual_cases.py`; those post-hoc
observations are not fed into this model's features or fitting.
