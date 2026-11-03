# Resident ridge responses, with joint services kept on

This continues the ridge-predictor path on branch `codex/resident-ridge-context`.
It runs the **real saved Raytrace Pass-3 workload**, with the existing joint
closure and shared loop-context services **enabled on both sides**. It does not
use the unrelated 9.87-second predictor-disabled configuration as its baseline.

The new backend is implemented and verified, but **has not improved complete
inference time** in the single measured pair. It is opt-in, not a replacement
for the repository's default egglog engine.

## The matrix and the persistent state

The previous predictor encoded each requested signature into 256 coordinates,
then multiplied those coordinates by the imported ridge weights. Let:

- `Z` be the active existing Analyze/spine features for a typed binding key;
- `C` be their existing signed coordinate codes;
- `W` be the unchanged FP16 ridge weights, stored with scale 256;
- `M` be the existing model-specific feature masks.

We precompute, **without refitting**, the response coefficients

```
A = M * (C W / 256)               (* is elementwise here)
R = Z [high(A), low(A), M]        (three interleaved columns per head)
score = (R_high + R_low) / d(R_count) + original_bias
```

`d(n)` is the original smallest power of two whose square is at least `n`.
The count is an exact integer within the admitted token bound. High/low FP16
coefficients avoid losing the small residual of one FP16 rounding. This preserves
the original real-valued model formula, with a different floating accumulation
order; it is not a claim of bit-identical scores for every possible input.
Predictions remain approximate and never authorize effects by themselves.

For the current library there are **101 input tokens**, including sort tokens,
and **25 heads**. Constant heads avoid matrix computation. The largest active
typed bank has 16 nonconstant heads, represented by 48 response columns.
Per-head masks apply to both the response numerator and its normalization count;
an excluded target feature cannot leak back through normalization.

The GPU retains private buffers for token coordinates, context versions,
intermediate matrix responses and normalized score banks. Native commit/rebuild
notifications still drive the existing CPU marker publisher. The sender assigns
stable `(sort, native value)` context handles and sends only changed feature
assignments at demand boundaries. It does not resend the expanded 256-coordinate
vector. Typed identity is kept separate from learned numeric features.

Several condition heads use the same context row. A changed context computes
its nonconstant typed bank; a later request for another head can read that
resident result without feature upload or another GEMM. Version mismatch is an
error, not permission to use stale scores. Buffer growth preserves prior GPU
rows; a changed version invalidates its score bank.

**These are prediction contexts, not a count of live RipSatCells.** Full exact
predicate witnesses, binding assembly and RHS/union/rebuild remain in the
existing CPU/native paths. This does not yet turn an entire Cell's inference
into matrix execution, and it does not inherit the separate small Math
prototype's autonomous A→B→C transition claim.

## One runtime, explicit ownership

`combined.rs` preserves the existing joint/loop providers and supplies remaining
queries through the ridge provider. A rule query has one owner at each Run.
The native commit journals are draining queues: the first observer forwards
shared rows to the second observer before forwarding each drained batch once
to the ordinary matcher. Each subscriber maintains its own union cursor.
Pre-initialization history is represented by the ridge view's current-state
bootstrap, rather than replayed twice.

This retains ordinary Run frontiers and the original checked saturation scopes.
The shared loop programs remain installed. The current model vocabulary is
unchanged; enabling the loop service is not a claim of newly trained loop-marker
features or newly discovered Cell proofs.

GPU/audit runs require `predictor_loaded=true` and actual ridge matrix work.
`groups`/`ready`, which deliberately bypass prediction, now require an explicit
`--allow-idle-predictor` control. The no-inference fixture proves that loading a
model alone cannot produce a successful predictor benchmark report.

## Measured result

Same binary, model, source, ordinary GPU matcher, joint/loop services and exact
completion; each timed lane ran once. Original native controls were reused.
Setup, compilation, export, validation and extraction are outside inference.
The predictor device timer is nested inside round-trip and provider timers.

| Metric | Shared packet coordinates | Resident response bank |
|---|---:|---:|
| Complete inference | 14.1441 s | 14.2114 s |
| Setup | 3.1668 s | 3.3035 s |
| Predictor device execution | 5.562 ms | 3.279 ms |
| Predictor round trips | 50.285 ms | 51.118 ms |
| Exact provider maintenance + queries | 1.2202 s | 1.2423 s |
| Requested new scores | 827 | 827 |
| Request batches / matrix banks | 40 / 41 | 40 / 41 |
| Upward predictor protocol bytes | 56,288 | 64,004 |
| Exact CPU residual reads | 708,814 | 708,814 |

The device arithmetic phase is lower, but its complete round trip and total
inference are not. Do not turn this
into a claim of end-to-end acceleration or the requested 2× result. Byte counts
are Rust/worker protocol bytes, not physical GPU-bus traffic.

Resident state/work:

- **438 typed prediction contexts**, **549 context updates**, **5,582 feature
  assignments**; only contexts actually demanded are allocated.
- **20** later head requests reused an earlier bank result.
- **8,769 heads computed** versus **827 requested scores**. At the end, **6,355**
  cached heads had not been used; **1,587** earlier heads were invalidated unused.
  This overcomputation is explicit speculative cost, not useful-match credit.
- **314,368 bytes** of allocated private context buffers; excludes immutable
  models, host mirrors and the original graph. It is not an RSS compression ratio.
- Physical matrix output cells grow from 5,568 to 36,160. The new contraction
  width is 101 (padded internally to 104), versus the old 256; response columns
  include high/low/count components. Raw cell counts are not equal work units.

The remaining issue is concrete: computing the whole typed bank is much broader
than the head demand actually reused. A useful next design should derive head
scope from the existing consumer/port registrations, retaining growth when a
new consumer arrives. Wider matrices alone are not progress.

## Verification and provenance

- **32 Rust tests** and **2 exhaustive small-mask lowering tests** pass.
- **703 GPU score comparisons** against the old FP16-coordinate reference:
  maximum error **1.20e-7**, no threshold disagreement in those tests.
- Additional GPU mask test: adding excluded features leaves that head unchanged.
- Growth, sparse removal/addition, same-row multi-head reuse and stale-version
  rejection pass; a required-but-idle predictor is rejected.
- **156,426 independent native incremental binding checks** pass for queries
  supplied by the combined services. This is not an audit of every fallback
  GPU query or every internally certified closure step.
- All original facts, values, equality and visibility match: **699,068 rows** and
  **524,631 mapped e-classes**. Both timed variants emit **2,768,828** complete
  original bindings, with identical per-rule counts.
- Production Tiger chooses the same DAGs for all **29 functions** (8,007 selected
  constructor rows) against the same-submit CPU control. This is not pristine
  upstream/fused-native parity.

`resident-results.json` contains the compact measurements, per-rule exact-check
curves, numeric checks, verification and build/model provenance. Final timed outputs
are `out/ridge-resident-final-shared` and `out/ridge-resident-final`. The earlier
14.2488/14.3214 s pair is retained under `out/resident-response-*`; the final pair
uses the corrected 16-byte Metal constant-argument ABI and is not a best-of pick. The existing model was trained
from the saved graph of the same workload; this is not a held-out program claim.
Read-only comparison/reporting edits do not change the measured algorithms;
actual binary and model hashes are retained. Metal API validation was enabled
for the final numeric test, not for the timed runs.

## Reproduce

Use the existing NumPy Python environment and the existing benchmark helpers:

```sh
python experiments/structural_runtime/lower_ridge.py \
  --model out/structural-runtime-build/matrix-model.json \
  --output out/resident-response-model.json
python3 experiments/structural_runtime/build.py \
  --runtime-repo "$RUNTIME_REPO" --output out/resident-response-build
python3 experiments/structural_runtime/run.py \
  --runtime-repo "$RUNTIME_REPO" --build out/resident-response-build \
  --source "$PASS3" --output out/new-resident-run \
  --coordinates markers --matrix-layout resident \
  --model out/resident-response-model.json --query-worker "$QUERY_WORKER" \
  --joint-sat --boundary-programs "$SHARED_LOOP_PROGRAM" \
  --reuse-native out/structural-runtime-pass3-v4 --lanes gpu,audit --extract
```

For the paired control change only `--matrix-layout shared` and output directory.
Then use `--compare-resident CONTROL_RUN`; it verifies predictor activation on
both sides, matching service configuration, input/tool identity and complete
binding/graph gates. Do not compare these times directly to the 9.87-second
predictor-disabled optimization experiment.
