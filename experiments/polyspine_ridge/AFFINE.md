# Fixed affine columns alongside Polyspine ridge

Follow-up: [resumable marker contexts](MARKER_CONTEXT.md) preserve missing-input
links and iterate GPU output columns into downstream contexts.

This is an **offline experiment on real native Raytrace Pass 3**, not an engine
speedup. It preserves the scale experiment's input, schedule, labels, root
lineages, 512-wide ridge input, regularization 1, and threshold 0.5. The 12 actual
`terms` saturations contribute 262,847 root observations. Only their PRE-stage
cost contexts were recaptured; the other stages' experiments were not repeated.
All 10,121 native Run update flags and all saved terms gain/loss labels agree.

## Result

Same-program held-out union lineages: 53,073 observations, **2,309 positive
observable `ExtractedExpr` payload updates**, 209,774 training observations.
These are not 2,309 independent programs, full matches, or extraction-quality
improvements. Graph neighborhoods can overlap across held-out root lineages.

| Input | TP | FP | Precision | Recall |
| --- | ---: | ---: | ---: | ---: |
| Previous versioned Polyspine, reused weights | 994 | 58 | 94.49% | 43.05% |
| + actual child/old costs and availability | 982 | 49 | 95.25% | 42.53% |
| + rule-compiled candidate cost / margin summaries | 982 | 49 | 95.25% | 42.53% |
| + exact comparison / missing-old / validity masks | **1,037** | **24** | **97.74%** | **44.91%** |
| New columns shuffled within each entry snapshot | 994 | 58 | 94.49% | 43.05% |

The complete version newly recovers 58 positives and loses 15 previously
recalled ones: **43 net, +1.86 percentage points recall**. No threshold tuning
was performed. This is a modest benefit from the combined representation, not
evidence that affine RHS values alone solve the prediction problem.

The chronological split (cycles 0–5 train, 6–11 test) also remains difficult:
precision **20.32% → 25.96%**, recall **37.97% → 41.79%** (1,151 positives).
These features have not solved generalization across later stages.

## What actually executes

`affine_terms.py` reads the native typed rules. It discovers eight cost recipes,
rather than hardcoding costs by constructor name. Their coefficients are:

```
Const, Arg: 1
Bop:       1 + c1 + c2
Top:       1 + c1 + c2 + c3
Uop:       1 + c1
Get, Single: c1
Concat:      c1 + c2
```

`AFFINE_PROGRAM.json` records the exact selector and instruction programs from
this source. It is a numeric projection of these rules, **not a complete closure
certificate**. Other rule effects are not claimed to be implemented by it.

The diagnostic adapter aligns actual anchor rows, `ExtractedExpr` results and
correlated `TCPair(term,cost)` alternatives on CPU. A context retains its root,
recipe, three cost lanes, old cost, complete-input flag and old-answer flag.
Alternatives are not merged into independent term and cost marginals. Missing
inputs remain in the root population; their arithmetic is masked out.

On GPU:

1. Gather the root's existing 512-wide H and aligned parameters.
2. Put the parameters into the recipe's four fixed columns, with a shared old
   cost column: eight recipes fit a padded **552-wide** input. This is simple
   fixed wiring for this experiment, not an optimized sparse/matrix layout.
3. One FP16-input/FP32-accumulator SIMD-group matrix multiplication produces
   `[old ridge score numerator, cnew, cold-cnew]`. Learned and fixed coefficient
   blocks are stored separately when building the matrix. Fixed coefficients
   never enter training. The original FP32 ridge bias is applied after scaling.
4. Produce exact comparison and validity masks. Range-certified arithmetic
   feeds the GEMM output columns directly to subsequent pooling. Other tuples
   use checked GPU i64 arithmetic. Audit mode additionally checks **every**
   tuple against its original integer instruction order.
5. Reduce correlated tuple results into root summaries on GPU, then score the
   augmented ridge model with the existing GPU scorer.

Step 5 is a subsequent computation: merely appending fixed output columns to an
unchanged model cannot change its score. New nonlinear comparison results
cannot feed the same GEMM which produced them.

The fixed value columns are exact integers before feature encoding. Ridge gets
bounded signed `log2(1+abs(value))/64` summaries, counts/availability, and masks
in 24 previously unused columns. Raw-cost, derived-value and mask ablations
have identical total width. Min/max/mean summaries are **prediction features**;
they must never be combined back into a synthetic RHS witness.

An important limitation of attribution: affine expressions alone add no linear
expressive power beyond their operands. Here the derived features also include
log scaling and reductions across alternatives. Even with these additions,
the value-only variant did not improve held-out recall.

## Integer and semantic checks

- 362,879 aligned or incomplete contexts across the 12 snapshots.
- 123,355 have complete valid arithmetic; 114,758 fit the matrix range and
  8,597 require i64 fallback (including the real `10^16` sentinel).
- All exact values, masks, root reductions and matrix-derived safe values agree
  with the independent CPU reference. The fused old scores and all ten ridge
  model jobs have **zero threshold disagreements** with CPU scores.
- Large sentinels, negative values, missing inputs, equality/alias constraints,
  multiple correlated witnesses, intermediate i64 overflow, and overflow of
  `cold-cnew` with a still-valid candidate have regression tests.
- A native `.egg` test checks compiled cost against actual emitted `TCPair`.
- The full Polyspine suite passes **31 tests**, including native and Metal tests.

A cost-improvement condition controls an update helper, **not all candidate
creation**. The original `terms` rule can emit a non-improving candidate which
another rule observes. This experiment does not suppress those effects.

## Why recall is still low

Of the 1,272 remaining held-out false negatives:

- **756** have no complete cost context at stage entry. A needed child answer
  can be produced later in the same saturation. One-step arithmetic cannot
  compute a parameter that has not arrived.
- **516** already have a complete context. Completeness of one numeric tuple
  does not decide whether the final candidate/comparison payload changes.
  Other alternatives, term identity, unions and repeated propagation remain.

Only 247 of the 2,309 positive labels coincide with a decrease of an already
available minimum cost. The other 2,062 include first answers and payload
changes without such a decrease. A score for "cost gets smaller" is therefore
not the entire target. This is not a proof that all 756 missing-context cases
are recoverable by numeric propagation, nor a recall ceiling.

The next justified extension is to retain child-result links and iterate
computed output columns into parent contexts, with availability masks. It
must also retain candidate identity and comparison state: scalar min-cost
propagation alone is not the whole native closure. That extension is **not
implemented in this experiment**.

## Costs and scope

Saved capture reports roughly 2.00 seconds of **CPU tuple alignment** across
12 entries. It is not free and does not yet run on GPU. The fixed-context
matrix/integer/pooling work takes only milliseconds, but excludes capture,
alignment, training and actual engine effect submission; it is not an
end-to-end speedup or a throughput benchmark. Audit also duplicates safe
arithmetic in integer form deliberately.

See `AFFINE_RESULTS.json` for complete split/subset counts, device checks,
source hashes and diagnostic timings. Binary packets and the compressed
observation journal remain under `out/` and are not committed.

## Reproduce, or reuse the cached controls

Use Python with NumPy; the existing environment is:

```sh
PY=/Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python
mkdir -p out/polyspine-affine-build
xcrun swiftc -O experiments/polyspine_ridge/affine_worker.swift -o out/polyspine-affine-build/worker
```

Only recapture if the saved numeric context is unavailable. Existing output
directories are intentionally not overwritten by the Python entry points.

```sh
$PY experiments/polyspine_ridge/affine_capture.py \
  --dataset out/polyspine-scale-data \
  --workload out/matrix-dynamic-perf-input/workload.json \
  --frontend out/polyspine-scale-build/frontend --output out/polyspine-affine-data
$PY experiments/polyspine_ridge/affine_fit.py prepare \
  --dataset out/polyspine-scale-data --capture out/polyspine-affine-data \
  --oldfit out/polyspine-scale-fit --output out/polyspine-affine-projection \
  --worker out/polyspine-affine-build/worker
$PY experiments/polyspine_ridge/affine_fit.py fit \
  --dataset out/polyspine-scale-data --capture out/polyspine-affine-data \
  --oldfit out/polyspine-scale-fit --prepared out/polyspine-affine-projection \
  --output out/polyspine-affine-fit --worker out/polyspine-residual-build/score-worker
```

Tests:

```sh
POLYSPINE_AFFINE_WORKER="$PWD/out/polyspine-affine-build/worker" \
POLYSPINE_NATIVE_FRONTEND="$PWD/out/polyspine-scale-audit-build/frontend" \
POLYSPINE_SCORE_WORKER="$PWD/out/polyspine-residual-build/score-worker" \
POLYSPINE_RESIDUAL_WORKER="$PWD/out/polyspine-residual-build/worker" \
$PY -m unittest discover -s experiments/polyspine_ridge -p 'test_*.py'
```
