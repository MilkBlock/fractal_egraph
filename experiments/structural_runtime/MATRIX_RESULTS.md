# Shared spine rows and condition columns in the real runtime

This is a layout change to the live Pass-3 speculative query provider. A matrix
column is a **typed, slot-aware existential query projection**, not necessarily
a whole rule. Multiple original rules consume the same column. For instance,
Bop, If and Concat consumers can all ask ExprIsPure for their respective input
bindings. ContextOf, HasArgType, tuple-length and Expr-size are distinct columns;
the numeric-function columns predict whether a row exists, not its numeric value.
The exact CPU side still retrieves the full correlated rows and original values.

The `--residuals all --min-atoms 2` export admits 75 always-run rules with 25
condition columns. Admission retains the checked anchor-complete, guard-free,
single-bound-key contract. Rules outside it keep the ordinary GPU matcher.
This does not make every Pass-3 rule eligible, and does not import a saturation
certificate or silently skip primitive guards. The same saved source, schedule,
model, exact caches and native RHS are used in the two layout lanes.

## Representation and actual computation

```text
original rule requests
    -> shared exact condition blocks (projection, key, version)
    -> unknown conditions whose order can affect exact checking
    -> deduplicate immutable spine signatures across models
    -> encode shared rows X [N, 256] once
    -> multiple typed condition columns W [256, C]
    -> scores X W + bias
    -> select only the requested matrix positions
    -> exact CPU checks / complete bindings / original native RHS
```

A coordinate is the existing signed encoding of Analyze-derived spine features.
Two equal feature signatures share a prediction, never native identity, proof or
binding payload. CPU routing retains original eclasses and slot assignments.
Feature masks for source-summary markers are applied before interning rows, so
incompatible target masks do not accidentally share their unmasked input.

Binary protocol v2 sends each signature once plus `(row, model)` routes. The
worker uploads all learned columns once, then uses row and column maps to address
actual requested blocks. These are Metal simdgroup matrix operations; they are
not a measured NVIDIA Tensor Core result. No numerical model is retrained by
changing the layout.

For each typed request group the worker evaluates three finite layouts:

1. One column per model (control).
2. Rows grouped by identical requested column sets.
3. One rectangular bank for the currently requested rows/columns.

It chooses the smallest **actual 8x8 tile count**, then the fewest logical output
cells. This guarantees no additional matrix tiles relative to the column control
within these choices, not globally optimal wall time or arbitrary optimal sparse
matrix packing. Extra rectangle results are discarded; padding and extra results
are reported separately from requested scores. The kernel is not allowed to use
unrequested outputs as evidence about the graph.

Two exact simplifications prevent fake density:

- A zero-weight model returns its bias without coordinates, transfer or GEMM.
  This is a constant *prediction*, not certified predicate truth. CPU completion
  is unchanged.
- With at most one distinct unknown condition, a model cannot improve check
  order. Those requests bypass prediction. Cached exact-negative answers also
  retain their existing bypass. Thus enlarging the supported query set cannot
  be credited merely for feeding obvious or order-insensitive tasks to GEMM.

The backend remains optional (`--matrix-layout shared`). The default is the
single-column layout. The exported model's scope and the complete supported
query set must be identical for a layout comparison.

## Validation and reproducibility

Use the existing `export.py` source/snapshot/profile/Analyze arguments, adding
`--residuals all --min-atoms 2`, and save a separate model file. Build using the
existing `build.py`. Run `run.py` with that same `--model`, once for
`--matrix-layout single` and once for `shared`. Native and ordinary-GPU controls
can be reused explicitly. Use the `audit` lane only as validation, and request
`--compare-layout SINGLE_RUN` when summarizing the shared lane. Completed lanes
are fingerprint-cached; the comparison is read-only. `--extract` invokes the
existing bench skill's production Tiger and selected-DAG checks.

`check_gpu.py` checks both layouts against FP16-input/FP32 CPU reference scores,
including duplicate requests, repeated coordinates, multiple sorts, more than
8 columns and tail tiles. Test predictions are not timed benchmark claims.
The focused Rust suite retains late facts, replacement, multiple witnesses,
union, subsume, marker correlations, and complete CPU-complement regressions;
new tests cover shared packet routing and constant-model bypass.

The first exploratory pair in `out/structural-matrix-{single,shared}` still
included constant and single-unknown predictions. Its seemingly dense 15-column
blocks therefore are not the final useful-work result. It is retained for
provenance and never substituted for the final comparison.

## Final result after eliminating unused score work

Both layouts ran once with the same binary, model, original GPU worker and full
saved schedule. All 75 original queries and every supported binding remain.
The report is `matrix-results.json`; output directories are
`out/structural-matrix-useful-{single,shared}`. Historical native/ordinary-GPU
controls were reused, not retimed. Earlier exploratory measurements above are
not substituted for these final lanes.

| Measured matrix work | Single column | Shared columns |
|---|---:|---:|
| Newly requested nonconstant scores | 827 | 827 |
| Distinct coordinate rows encoded across batches | 500 | 500 |
| Requested cells in multi-column blocks | 0 | 803 |
| Matrix blocks | 129 | 41 |
| Maximum number of columns in a block | 1 | 7 |
| Logical matrix cells, including unrequested rectangle outputs | 827 | 2,730 |
| Physical 8x8-tile cells, including padding | 12,032 | 5,568 |
| Requested fraction of physical cells | 6.87% | 14.85% |
| Predictor GPU time (encoding + matrix) | 6.34 ms | 7.94 ms |
| Predictor round-trip time | 51.20 ms | 53.44 ms |
| Full inference | 22.113 s | 22.140 s |

Shared layout performs **53.72% fewer padded matrix cells** within the offered
layouts. Its 1,903 unrequested rectangle outputs are discarded and explicitly
excluded from useful-score counts. There is no measured speedup: the batches
remain tiny, and regularizing the tile geometry did not reduce measured device
or full-runtime time. No claim of high GPU occupancy or neural-network efficiency
follows from these shape numbers.

Both lanes already share packet coordinates, so 827-to-500 row reuse is common
infrastructure, not extra credit for the shared-column switch. Constant-model
folding bypasses 1,812 score requests. Another 1,674,954 rule anchor visits have
at most one distinct unknown condition and cannot benefit from score-based
probe ordering; they are not pushed into GEMM to inflate its size. There are
23,710 anchor visits with multiple unknown conditions. Exact cached answers
and repeated score signatures further shrink the new device work.

Meanwhile provider maintenance plus CPU query handling costs about **3.22 / 3.24
seconds**. Exact checking/binding work is unchanged between layouts. The new
75-rule provider is also slower than the unchanged historical ordinary-GPU
control (17.714 s), which uses a different query execution path. That observation
is retained, not presented as a paired matrix-layout speedup. Defaults therefore
remain unchanged: both the provider and expanded query export are optional.

The concrete result is a functioning shared-coordinate/condition-column
runtime, plus a negative performance finding: **batching score calculations
alone does not move the expensive truth checking and binding assembly into the
matrix**. More consumers and wider zero-filled or irrelevant matrices would not
fix that. Further work must let shared interfaces deliver more of the actual
matching result, while preserving the exact verification/completion contract.

Validation passed: **22 Rust tests**, both matrix layouts against **2,775 CPU
reference scores each** (maximum error 1.20e-7, no threshold disagreement),
**134,850 native incremental full-binding query checks**, exact final **699,068
original rows** including functions/equivalence/visibility, and production Tiger
selected DAG equality for **29 functions**. No original table is projected away.
This is the actual assisted native runtime under the existing common-submit
protocol, not pristine upstream parity or a new saturation proof.
