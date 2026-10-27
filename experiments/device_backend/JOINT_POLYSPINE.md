# Resident Polyspine program across the three terms rulesets

Follow-up: [exact phase-local feature reuse](FEATURE_REFRESH.md) preserves the
model and reduces repeated feature work. The initial implementation's timings
below remain as historical evidence.

`--joint-polyspine` recognizes the original nested schedule:

```text
saturate(terms; saturate(terms-helpers; saturate(terms-helpers-helpers)))
```

It acquires structural interfaces once, runs all three stages against one
GPU-private context, and publishes the complete resulting graph at the outer
Saturate boundary. `--polyspine-model FILE` additionally enables the learned
Polyspine heads on that same context. No native CPU matching, RHS execution or
binding-array handoff is used in normal execution.

This is connected to the actual device backend. It is a certified program for
the existing terms ABI, not a compiler for arbitrary collections of rulesets.
Ordinary Run, a differently ordered schedule, unsupported effects and rejected
runtime preconditions retain the ordinary execution path.

## Reused analysis and Interface components

- `polyspine_ridge.resumable_program.compile_links` is the shared compiler
  extracted from the previous marker-context analyzer. Both the analyzer and
  this runtime consume its typed ports, unresolved dependencies, complete heads
  and original integer-operation DAGs.
- `tools.cell_spine.interfaces.native_row_interface` uses the existing Interface
  coordinate mechanism to describe complete structural argument tuples. It
  retains the root, input positions and owner/generation contract.
- `joint_polyspine.compile_contract` additionally verifies **all** seed/helper
  effects, initializer, comparison cases, function merge, schema feedback and
  the nested schedule. The older minimum-cost projection is not accepted as a
  full graph certificate.
- `polyspine_ridge.study.fit_model` is the existing ridge fitter. The runtime's
  signed codebook uses the same fingerprint/SplitMix bits as
  `superposed_key_memory.memory.codes`; the numerical audit checks this exactly.

The current seed compiler supports the eight real Raytrace terms recipes. It
does not import arbitrary solver-selected compound Summary `.egg` layouts.
Its composite program includes the subsequent helper and comparison structures,
and shared child contexts connect root and non-root positions.

## Three representations, one resident context

1. **Exact structural interfaces.** Every actual source occurrence has a marker
   with its owner fact, root, recipe and typed child coordinates. An unavailable
   child remains a port. Multiple structures returning the same root remain
   separate binding records; root-level feature pooling cannot combine their
   arguments into an invented RHS.
2. **Exact evolving payload.** A private row-store view retains real Terms,
   every TCPair alternative, PotentialExtractedExpr, ExtractedExpr, Smaller,
   union-find, timestamps and fact identities. Complete-key hashcons is reused.
   Generated records directly publish local read ports for later stages. Cost
   witnesses are stored as whole `(Term,cost,return)` rows, not marginals.
3. **Floating context matrices.** Structural heads, roles, neighboring Spines,
   prior-stage properties, readiness, cost ranges/bytes and update state produce
   `Phi`. Fixed signed encoding `E` produces root coordinates; role-bound neighbor
   coordinates add a second Polyspine block. One shared model supplies three
   forecasts: remaining terms effects, helper effects, and comparison effects.

The representation groups actual root contexts and preserves their binding
occurrences. It does not allocate one instance per global row × every Bake plan.
Dynamic lists and job matrices grow to observed needs; storage is reused, but
old logical context contents are not silently reused as proof.

## Results directly feed later inference

```text
marker + child coordinates
    -> complete correlated child Term/cost tuples
    -> fixed affine matrix / checked i64 arithmetic
    -> real Term + TCPair + Potential records
    -> local helper binding and ordered function update
    -> Smaller records
    -> exact byte-matrix comparison and private union closure
    -> updated ExtractedExpr/cost ports
    -> previously pending parent marker becomes ready
```

Only the first interface acquisition scans the structural graph. Later stages
read persistent ports, local row lists and cost-witness CSR. New rows are
registered from the just-committed range, without searching for their source
structure again. Comparison reuses the current class extrema for its initial
iteration; subsequent union iterations refresh them locally.

The private backing view is currently a copy of the published GPU row store,
not a minimal compressed Cell payload. The entire view is published once at
the exit. This is an explicit memory/copy cost and is reported separately from
the floating matrices. Necessary RHS/hashcons materialization remains ordered
on GPU; it has not been replaced by a parallel constructor allocator.

## Fixed computation and learned proposals

The rule-compiled affine coefficients are immutable. In the certified numeric
range, the matrix result is put into the actual RHS register stream. Every
original intermediate i64 operation is still checked, so reassociation cannot
hide overflow. Large values use exact GPU integer arithmetic. There is no
clamping to the initializer and no suppression of non-winning candidates.

Comparison uses the exact signed-i64 byte encoding from
[MATRIX_COMPARISON.md](MATRIX_COMPARISON.md). Both min/max existence conditions
may hold simultaneously. The original evidence rows remain available.

The learning representation has 64 encoded root columns and 64 role-bound
neighbor columns, plus bias/padding to width 136. With the eight real recipes,
the base feature width is 72. It includes exact guard summaries and byte
coordinates, not opaque eclass IDs treated as numbers.

Learned coefficients are loaded once. Model compatibility binds the typed rule
program, Interface layout, feature schema and fixed encoding checksums. A model
cannot replace the fixed encoder or effect recipes. It proposes contexts for
the first work pass; all remaining contexts run through exact fallback before
that stage commits. Already checked bindings are not checked again in fallback.
The RHS ordering is restored independently of prediction priority.
This version prioritizes contexts; it does not yet skip the exact work in the
negative-prediction partition. Prediction alone therefore cannot claim avoided
Join work or avoided effect validation.

Predictions are **not** assertions about facts and never determine phase
visibility, guards or the fixed point. Tests deliberately use all-negative,
all-positive and exactly-at-threshold models. A forecast can be wrong without
losing a required fact or creating an invalid one.

## Schedule, identity and failure contract

The GPU phase state follows the exact nested schedule. A helper phase reads its
entry state; results become available only to the next permitted phase. A
comparison result can therefore enable another helper iteration and then a
parent terms iteration. It cannot move a future result into an earlier Run.

Each complete tuple is sorted by the original compiled query's rule/pivot,
timestamp, seed and witness ordering before the existing typed RHS sink runs.
This preserves order-sensitive function merge behavior and all observable
intermediate constructors. Internal phases are recorded separately; they are
not reported as one original logical match.

Stage advancement is encoded after the device effect sink, on the same command
buffer. The host still dispatches preparation and variable-size work batches;
this is not one persistent kernel with no internal host synchronization.

The context is refreshed at each outer Saturate entry. External union,
visibility changes and retired source owners are therefore seen before reuse.
Within a certified scope the structural carrier cannot change; mutable records
are versioned and duplicate records are retired immediately. Incremental
maintenance of these interfaces across *all* external rulesets is future work.

Private errors reject the macro before publication. The original executor then
preserves the original error behavior. Direct protocol tests check that rejected
overflow and panic scopes leave published rows, IDs, parents and metadata
**byte-identical**. Original device row/match/identity capacities remain explicit;
no truncated closure is accepted.

## Verification and attribution

The regression suite checks complete native tables, primitive/function values,
visibility and anchored equality partitions. It includes multi-level trees,
multiple Spines at one root, correlated alternatives, aliased ports, pending
comparisons, unfounded cycles, external unions, retired owners, finite Run entry
states, non-winning candidates, large/signed integers, overflow, panic, stale
models and checkpoint restoration.

The real Raytrace initial nested terms scope is checked against an independent
native result. The complete Pass-3 endpoint is compared with the previously
checked GPU backend; its pre-existing whole-Pass native difference is not hidden
or repaired by this work. Source assertions still execute.

Metrics distinguish:

- structural marker bindings and root-context visits across invocations;
- a per-context three-bit service mask (terms/helper/comparison), which is not
  by itself net performance credit;
- real private constructor allocations, function value changes and surviving
  new rows after congruence;
- successful binding reads of another stage's version, and distinct
  `(row version, consumer stage)` uses;
- fixed-matrix RHS use versus exact integer fallback;
- learned proposals, exact fallback, private phase count, one final publication,
  retained workspace and GPU-to-GPU copying.

Training capture is diagnostic. Inputs are frozen before the captured phase;
targets count subsequent exact effects until the joint exit. Chronological
training/holdout scopes are explicit. The complete feature/score audit checks
Interface correlations, fixed encodings, Polyspine projection and actual GPU
ridge outputs. A diagnostic Interface-export bug was caught on full Raytrace:
payloads were read after final compaction. The old failed capture is retained;
the corrected export freezes those payloads before compaction.

## Commands

```sh
python3 experiments/device_backend/build.py --worker-only \
  --reuse-native-build out/device-build-context-final-v2 \
  --output out/joint-build --target target/device-backend

MTL_DEBUG_LAYER=1 EGGFUSE_MATRIX_AUDIT=1 \
python3 experiments/device_backend/test_joint_polyspine.py \
  --build out/joint-build --output out/joint-tests

python3 experiments/device_backend/run.py --source /path/to/pass-3-flat.egg \
  --build out/joint-build --output out/joint-capture --joint-polyspine \
  --matrix-saturate --polyspine-training --check-every 0 --save-state \
  --capacity 1048576 --match-capacity 1048576

python3 experiments/device_backend/compare_runs.py OLD_CONTROL out/joint-capture \
  --semantic-device-states --saturation-macros \
  --output out/joint-capture/verification.json

# PY is a NumPy-enabled Python. This uses the existing Polyspine ridge fitter.
$PY experiments/device_backend/train_joint_polyspine.py fit \
  --run out/joint-capture --train-scopes 6 \
  --verification out/joint-capture/verification.json --output out/joint-model.json

python3 experiments/device_backend/run.py --source /path/to/pass-3-flat.egg \
  --build out/joint-build --output out/joint-inference --joint-polyspine \
  --matrix-saturate --polyspine-model out/joint-model.json \
  --check-every 0 --save-state --capacity 1048576 --match-capacity 1048576
```

A fitted model for the recorded Raytrace program is checked in at
[`models/terms-polyspine-raytrace.json`](models/terms-polyspine-raytrace.json).
Its layout fingerprint must match; it is not a universal model for arbitrary
terms programs.

For a no-model control, omit `--polyspine-model`; it is explicitly reported as
such. Omit `--polyspine-training` for timing. Saved controls are reused rather
than rerun just to refresh a date. `summarize_joint_polyspine.py` verifies saved
endpoints and emits the fixed reuse/effect ledger.

## Recorded outcome, 2026-10-27

This integration is functionally verified but **does not establish a speedup**.
The old device control is a saved run; the two new lanes use the same new
worker, source, capacities and schedule. Each lane was timed once after the
final change, with feature capture and native checks off during inference.
Shader/frontend setup and subsequent graph checking are excluded.

| Full Raytrace Pass 3 | Inference | GPU command time |
|---|---:|---:|
| Saved device control, per-ruleset comparison matrices | 39.307 s | 30.476 s |
| Joint exact context, predictor off | 40.624 s | 29.210 s |
| Same joint context, fitted Polyspine ridge enabled | 44.341 s | 33.503 s |

Both new endpoints have **699,784 rows / 524,742 eclasses** and pass complete
typed graph, function/value, visibility and equality-partition comparison with
that saved device control. Both retain the original schedule. Internal Run
records differ because the certified comparison closure and the enclosing
joint scope are represented as macros, not fabricated original match events.
The independent native initial terms scope, with the real fitted model enabled,
passes at **27,793 rows / 21,655 eclasses**. The earlier full-Pass native mismatch
of this device backend is still outstanding; this is not a claim to fix it.

The shared context does real work across stages, identically in both lanes:

| Measurement | Count |
|---|---:|
| Interface acquisitions / outer publications | 12 / 12 |
| Internal terms / helper / comparison phases | 454 / 1,186 / 1,186 |
| Structural marker binding visits | 362,879 |
| Root-context visits | 226,147 |
| Context visits serving all three stages | 9,672 |
| Successful binding reads of another stage's results | 311,531 |
| Distinct row-version / consuming-stage uses | 184,564 |
| New rows surviving each private closure's compaction, summed | 277,308 |
| Actual function-value changes | 103,593 |
| Affine RHS rows supplied by the fixed matrix | 11,226 |
| Large-value rows using exact integer fallback | 52,982 |
| Intermediate binding bytes downloaded for CPU execution | 0 |

These are visits and effects, not distinct Bake families or net Cell credit.
**211,065 context visits served none of these stages.** This explains why
refreshing features for every context on every internal phase is costly.
The tracked retained workspace reaches 597,500,160 bytes (about 570 MiB),
including the transactional row-store view. It is not total RSS. View copies
sum to 12.28 GB of GPU-to-GPU traffic over the 12 invocations.

The model trains on the first six scopes, with 336,069 feature rows, and tests
on the later six, with 342,372 rows. This is one-program chronological holdout.
The three heads' held-out precision/recall are respectively **1.34%/6.40%**,
**42.88%/61.32%**, and **35.27%/25.72%**. They are not strong enough to justify
skipping exact work. Refresh/projection plus exhaustive fallback currently adds
cost, not a net search saving. Three pre-phase frames from the actual native
scope have zero observed discrepancy in the independent GPU encoding and
ridge-score audit.

The main remaining costs are explicit: repeated feature/CSR refresh over
inactive contexts, host-controlled variable-size phase dispatch, and ordered
GPU RHS materialization. The fixed context eliminates structural rediscovery
and repeated global publication; it has not yet eliminated these costs or
established better hardware occupancy. Both options therefore remain opt-in.

[The checked ledger](joint-polyspine-results.json) records source/build hashes,
configuration and attribution. [Validation inventory](joint-polyspine-checks.json):
34 new integration cases, 95 existing device/matrix cases, 39 analyzer tests
and three pure Interface tests passed. Metal API validation and matrix audit
were enabled for the device regressions. Three older Interface tests requiring
absent standalone release tools could not run; they are explicitly listed,
not counted as passes. Native union, retirement and correlated-binding behavior
is independently exercised by the new suite.
