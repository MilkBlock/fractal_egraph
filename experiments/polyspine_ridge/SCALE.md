# Polyspine at real Pass-3 stage scale

This scales the **dataset and local prediction problem**, not by copying a tiny
graph or counting repeated easy positives. The original native Pass-3 schedule
still executes every Run. All 182 visits to four selected saturation families
are observed, including inner revisits previously omitted by first-per-cycle
sampling. This does not cover every rule or every output of Pass 3.

## What is predicted

The task is an **observable answer gain/update** at each pre-existing Expr key:

- `HasType`: a new type payload, modulo the actual union relation.
- `ExprIsPure` and `ExprIsResolved`: a newly present unary answer. Already-present
  keys are retained in the data but excluded from this learned task.
- `ExtractedExpr`: a new/changed function payload, including the one-level
  constructor payload of its result (TCPair includes the candidate and cost).

For functions this is a payload-view change, not necessarily a physical new row
or a better extracted program. Comparison constructors such as Smaller can also
change this view without improving the chosen term. Final extraction quality is
not measured here.
Post-stage gains at keys outside the pre-stage domain are reported separately;
they are not silently called covered.

All features, existing answers and raw support rows are captured **before** the
stage. Future native facts supply labels only. Future union lineage is used
solely to keep aliases together in the held-out split, never as a feature.

## Two evaluation splits

1. **Time:** cycles 0–5 train, cycles 6–11 test. A root may recur over time.
2. **Union lineage:** deterministic 20% of final union components test, the
   remainder train, across all observed stages. All versions of a root that
   native union connects stay on the same side, across target families.

The second split tests unseen root lineages of this **same program**, with
transductive graph context visible. It is not an unseen-program or fully
disjoint-neighborhood test. Report eligible observations, positive updates,
distinct lineages and distinct positive payload-transition keys separately.
Repeated zero-update stages are real workload observations but not independent
examples; weighted ridge aggregates their identical coordinates exactly.

## Features and device computation

Fixed 512-wide coordinates and the previous ridge penalty/threshold are kept.
The comparisons are structural shape, entry-state local interfaces, overlapping
Polyspine interfaces, one/eight-step residual projection, versioned projection,
and shuffled residual correspondence. Before-answer presence/count and numeric
projection channels occupy explicit columns, not opaque identity codes.

The versioned model compares the current PRE-stage structural/interface
signature and answer count with the same root's earlier PRE-stage version.
No earlier label is cached as a feature. It uses observed raw root identities;
the future union-component split is not used to reconstruct history.

The existing GPU residual worker directly consumes raw row matrices and shared
rule column selectors for purity and resolved. Every projected frame is checked
against an independent CPU implementation, including readiness fractions. Type
and extraction do not have such a projection certificate; their learned
interface features remain speculative. The existing SPINE004 GEMM worker checks
every exported ridge score against the FP16/FP32 reference.

## Compact observation

The native observer exports only requested tables, interns typed values, and
sends row insertion/removal differences plus union lineage. It is read-only and
does not alter matching, RHS, merge, rebuild or scheduling. It still scans the
selected tables at checkpoints: it is a **diagnostic adapter**, not a production
commit-event implementation. Function changes and subsumption remain explicit.

The observer is tested against full native snapshots after union, set and
subsume. All original native Run update flags are checked during collection.
The exact binary and source are recorded; this is the local native kernel, not
the pristine upstream tag. No GPU result is committed to the egraph.

## Reuse and reproduction

Use the project's NumPy environment. The capture is reusable for new model
fits; do not repeat native execution just to change regression/layout choices.
Native compilation and GPU workers are separate from measured fit/evaluation.

```sh
python experiments/device_backend/build.py --release --frontend-only \
  --target /Users/mineralsteins/Repos/egg_related/check_statewalk/target \
  --output out/polyspine-scale-build

python experiments/polyspine_ridge/scale_capture.py \
  --workload out/matrix-dynamic-perf-input/workload.json \
  --frontend out/polyspine-scale-build/frontend --output out/polyspine-scale-data

# Reuse the already verified residual worker (or build it via RESIDUAL.md).
out/polyspine-residual-build/worker out/polyspine-scale-data/always-run/capture.json \
  out/polyspine-scale-projection/always-run experiments/polyspine_ridge/residual.metal
out/polyspine-residual-build/worker out/polyspine-scale-data/is-resolved/capture.json \
  out/polyspine-scale-projection/is-resolved experiments/polyspine_ridge/residual.metal

python experiments/polyspine_ridge/scale_fit.py \
  --dataset out/polyspine-scale-data --projection out/polyspine-scale-projection \
  --worker out/polyspine-residual-build/score-worker --output out/polyspine-scale-fit

python experiments/polyspine_ridge/scale_report.py \
  --result out/polyspine-scale-fit/results.json \
  --output experiments/polyspine_ridge/SCALE_RESULTS.md

POLYSPINE_NATIVE_FRONTEND="$PWD/out/polyspine-scale-build/frontend" \
POLYSPINE_RESIDUAL_WORKER="$PWD/out/polyspine-residual-build/worker" \
POLYSPINE_SCORE_WORKER="$PWD/out/polyspine-residual-build/score-worker" \
python -m unittest discover -s experiments/polyspine_ridge -p 'test_*.py'
```

Original fact tables, complete bindings, extraction and end-to-end speed are
not replaced or benchmarked by this experiment. Good projection/score accuracy
must not be presented as a Pass-3 acceleration result.
