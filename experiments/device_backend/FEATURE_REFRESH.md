# Exact phase-local Polyspine feature reuse

This changes feature maintenance in the actual GPU-owned joint terms program.
It does not change the learned model, its features, matching, RHS, union,
publication boundaries or the exact fallback. It is enabled when the joint
program uses a model or captures training features. No learned score determines
whether an input may be left stale.

## What is cached

The three internal stages have separate caches. Otherwise, changing the
one-hot stage columns would invalidate every context on every stage switch.
The codebook, ridge weights and feature-layout fingerprint remain unchanged.

1. Structural heads, recipe proportions, port counts, producer/consumer roles
   and frozen type/purity/resolution properties are prepared once per outer
   scope. These properties are immutable in the certified terms program.
2. A 16-word **exact input tuple** is compared with that stage's previous tuple:
   readiness; full i64 extrema; witness/candidate/choice counts; generation and
   effect counters; ready-child count; and the previous-phase change flag.
   It is not a hash. Primitive values are compared before byte encoding or
   float conversion. An explicit valid word forces initialization even for an
   entirely empty context.
3. Only changed tuples rebuild feature rows and their fixed `E` projections.
   A context's Polyspine output is also invalidated when any directly referenced
   child projection changes. This follows the existing feature wiring; it does
   not infer relevance from past successful effects.
4. Eight-row matrix tiles with no changed rows return before matrix work.
   Mixed tiles recompute up to eight rows; actual row lanes are counted, rather
   than equating one dirty row with one row of GPU work. Cached raw scores are
   scaled exactly once when copied into the current-stage score buffer.

Every outer joint scope acquires fresh interfaces and resets its caches, so
external union, retirement and newly visible structure cannot reuse a stale
previous scope. Cache capacity follows actual context count. With features
disabled, the feature buffers are not reserved for every root.

The inexpensive exact-input check still scans contexts and their child ports
each phase. Candidate summaries and cost CSR preparation also remain. This is
not yet direct event-only invalidation, and the counters expose those checks.
Contexts that never emit an effect may still supply features to a neighbor;
they are never discarded merely because their service count is zero.

## Recorded complete Raytrace Pass 3 result

Both lanes use the same source, model, capacities and original schedule.
The unchanged full-refresh control is reused from
`out/joint-polyspine-published-ridge`; the candidate was timed once, without
validation or training capture during inference. Setup/compilation and the
subsequent complete-graph comparison are outside these inference times.

| Measurement | Previous full refresh | Phase-local reuse |
|---|---:|---:|
| Inference | 44.341 s | **42.294 s** |
| GPU command duration | 33.503 s | **30.499 s** |
| GPU preparation | 9.108 s | **5.836 s** |
| Context-stage exposures | 55,152,361 | 55,152,361 |
| Actual base feature rows rebuilt | 55,152,361 | **1,063,029** |
| Polyspine rows rebuilt | 55,152,361 | **1,299,890** |
| Encoding matrix row lanes | 55,152,361 | **1,936,878** |
| Ridge matrix row lanes | 55,152,361 | **2,558,328** |
| Tracked retained workspace | 597,500,160 B | 639,612,672 B |

Inference decreases **4.6%**, GPU command duration **9.0%**, and base feature
reconstruction **98.1%**. The additional phase caches cost about **40.2 MiB**.
Workspace is neither total GPU allocation nor process RSS. Matrix row lanes
count existing rows in executed tiles; final padding lanes are excluded on
both sides. The previous 226,147 figure was root visits over scope entries,
not the number of feature refreshes across all internal phases.

The final graph remains **699,784 rows / 524,742 eclasses**, with complete
typed rows, values, visibility and anchored equality partitions equal to the
saved control. Its original schedule and recorded macro step history also
agree. See [machine-readable results](feature-refresh-results.json).

This is a gain over the enabled-predictor lane. It is still slower than the
historical joint predictor-off lane (40.624 s) and the pre-joint device lane
(39.307 s); it does not establish an overall advantage for ridge prediction.
The pre-existing complete-Pass native mismatch is unchanged.

## Verification and reproduction

`--audit-polyspine-refresh` independently runs the original full feature
constructor, projection, seam assembly and ridge GEMM at every private phase.
It requires exact element equality with the caches before inference continues;
a discrepancy rejects the private scope. Test acceptance checks must therefore
also require that no scope fell back. Audited runs are not clean timings.

The 36-case integration suite includes pending ports, later union, subsumption,
multiple Spines at one root, complete correlated alternatives, large i64 values,
phase switches, adversarial models, restore, overflow/panic rollback and forced
full refresh. The initial real Raytrace terms scope passes **11,717,280** row
audits and the independent native graph check at 27,793 rows / 21,655 eclasses.
The separate saved-frame NumPy audit also checks the actual learned scores.
An additional **complete Pass-3 audit** checks all **55,152,361** context-stage
rows against fresh GPU computation, with all 12 scopes accepted and no
fallback. Its complete step records and per-scope positive prediction counts
agree with the clean run and historical control. This instrumented execution
is excluded from the timing table. [Validation record](feature-refresh-checks.json).

```sh
python3 experiments/device_backend/build.py --worker-only \
  --reuse-native-build out/device-build-joint-published \
  --output out/refresh-build --target target/device-backend

MTL_DEBUG_LAYER=1 EGGFUSE_MATRIX_AUDIT=1 \
python3 experiments/device_backend/test_joint_polyspine.py \
  --build out/refresh-build --output out/refresh-tests --audit-refresh

python3 experiments/device_backend/run.py --source /path/to/pass-3-flat.egg \
  --build out/refresh-build --output out/refresh-run --joint-polyspine \
  --matrix-saturate --polyspine-model \
  experiments/device_backend/models/terms-polyspine-raytrace.json \
  --check-every 0 --save-state --capacity 1048576 --match-capacity 1048576
```

Use `--full-polyspine-refresh` for a forced-refresh correctness ablation; it
still uses the new static-column preparation. Use the saved old implementation
for the historical performance comparison. `summarize_joint_polyspine.py`
reports named refresh counters and checks complete saved endpoint graphs.
