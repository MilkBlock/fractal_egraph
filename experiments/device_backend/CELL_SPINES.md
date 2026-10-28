# Boundary Spine recipes in the actual GPU runtime

V3 extends the v2 candidate-update artifact with source-derived `terms` boundary
programs. Real Analyze observations select the admitted spines; the loader
validates their complete typed body wiring, constructor effects and frozen
arithmetic program against the current source. It does not infer a proof from
frequency or from node coverage.

The eight tested real Pass-3 anchors are Const, Arg, Bop, Top, Uop, Get, Single
and Concat. Their recipe parameters are the marker's actual fields plus the
complete child TCPair witnesses. Existing target results, multiple spines and
multiple child witnesses are permitted. This is not the v1 fresh/singleton
shortcut: there is no fresh-root guard or preemptive completion of a new Cell.

## Execution path

```
canonical marker + current exact child-port projections
    -> all correlated witness tuples at the original Run frontier
    -> compact typed recipe parameters
    -> existing affine GEMM + exact i64 validation
    -> original request ordering
    -> compiled constructor recipe
    -> v2 candidate updates and comparison closure
```

The existing port projection already establishes the structural joins. Covered
jobs no longer call `jp_bind` to reconstruct/check the whole source variable
vector. Their RHS no longer goes through the original action-bytecode
interpreter. Integer operations still use the shared exact evaluator: the
artifact's numeric DAG is validated against that evaluator, and only certified
small values enter the FP16/FP32 affine path. Large costs and overflow keep exact
i64 behavior. Node identities remain integer slots.

Constructor instructions use the existing constructor-prediction/write API,
including hashcons identity and visibility. Replacing it with an unrelated
constructor allocator could break a mixed batch's sharing with fallback rules.
The original delta pivot, seed, witness order and stage publication remain.
No source `.egg` rule or ordinary finite Run is rewritten by this execution mode.

A missing recipe leaves that rule on the original binder and RHS interpreter.
The partial-library test removes Top: 10 actions use recipes, 2 use fallback,
and the complete graph remains equal to native. This is complete fallback, not
sampling or dropping unobserved cases.

## What this saves and what it does not

This change removes redundant binding reconstruction and RHS interpretation
for a known interface. It **does not reduce the number of legitimate boundary
candidate tuples**. If two ports have two witnesses each, their four correlated
combinations still exist. The shared port projection, table publication and
remaining product enumeration are still work. No new GPU occupancy claim or
new dense-matrix representation is established by this experiment.

`spine_program` counters report candidate tuples, executed recipe actions,
fallback actions and tuples from multi-valued groups. Actions can be no-ops;
these counters are not committed mutations. Generic binder visits remain in
the existing terms/helper counters for direct comparison.

## Reproduce

The native training source/evidence from `CELL_UPDATES.md` can be reused:

```sh
python3 -m tools.cell_spine.executable \
  --analysis out/cell-update-pass3/analysis-v2/analysis.json \
  --core out/cell-update-pass3/training-reference/core.json \
  --source out/cell-update-pass3/training.egg \
  --output out/boundary-spines.json --spines
python3 experiments/device_backend/run.py \
  --source PASS3.egg --build BUILD --output NEW \
  --cell-program out/boundary-spines.json --stage-contexts --matrix-saturate \
  --check-every 0 --save-state --capacity 1048576 --match-capacity 1048576
```

`--spines` emits a v3 boundary/update artifact, not a v1 fresh-completion file.
V1 and v2 still load. The source environment and native old-value evidence
requirements remain. Duplicate recipes, forged wiring/arithmetic/effects and
missing observation evidence are rejected before GPU execution.

- `test_cell_updates.py --spines ...` uses real Pass-3 rules, adversarial inputs,
  stage audits, native graph checks and transactional failure tests.
- `test_cell_spines.py --build BUILD --evidence out/cell-update-pass3 --output NEW`
  tests all eight recipes, partial fallback and corrupted artifacts.
- `extract_pair.py --reuse-control PREVIOUS_EXTRACTION ...` can reuse an unchanged
  control extraction. It checks source identity, Tiger identity, exact typed
  input rows and the saved output hash; the candidate is freshly extracted and
  the strict selected-DAG checker still runs.

## Recorded result, 2026-10-28

Actual complete Raytrace Pass 3, one run per setting, same frontend/worker,
original schedule, v2 candidate updates enabled on both sides, no ridge, stage
contexts and matrix saturation enabled. Only the boundary recipe mode differs.
Build/setup, export, graph validation and extraction are outside inference time.

| Measure | V2 updates only | V3 boundary + updates |
|---|---:|---:|
| Full inference | 42.9194 s | 42.3360 s |
| Joint terms GPU time | 6.2528 s | 5.3924 s |
| Joint terms control wait | 11.7163 s | 10.8855 s |
| Generic terms binder visits | 81,104 | 0 |
| Recipe candidate tuples | 0 | 81,104 |
| Recipe actions | 0 | 81,104 |
| Fallback actions | original path | 0 |
| Generic helper binder visits | 0 | 0 |

Observed paired inference improvement: **1.36% (1.014x)**; the joint terms GPU
part improves **13.76%**. This is a small measured improvement, not a claim of
large or repeatably stable end-to-end acceleration. Both new runs are slower
than the previous 39.9954 s v2 record. That shared absolute-time difference is
unattributed; the paired ablation does not explain it or erase it. The feature
therefore remains explicitly selected by the loaded artifact.

Validation:

- Complete off/on graphs: **699,784 facts / 524,742 eclasses**, exact anchored
  equivalence, function values and visibility; original and internal Run
  schedules equal.
- All eight real recipes: native equality, **214 rows / 182 classes**.
- Partial library (Top absent): the same native graph, with fallback exercised.
- Correlated multi-valued inputs, late union, multiple spines, finite Run,
  initializer-sentinel entry, mixed v1/v2 programs and rollback are checked.
- Production Tiger: **29 functions / 8,019 selected rows**, strict selected DAG
  equal to the previous verified GPU endpoint. The unchanged control extraction
  is reused after exact input/tool checks.
- The existing full native CPU/device difference is not repaired or hidden.

Results: `out/cell-spines-pass3-{control,full}`, `out/cell-spines-real-tests-v2`,
`out/cell-spines-coverage-tests`, `out/cell-spines-extraction`; compact committed
metrics are in `results/cell-spines.json`.

The next substantial opportunity is reducing repeated context refresh and
stage dispatch or compiling multiple transitions together. Replacing 81k small
binders alone cannot remove the other rulesets' search or the remaining global
publication/rebuild cost. This patch supplies executable boundary recipes for
that work; it does not claim those larger optimizations are already done.
