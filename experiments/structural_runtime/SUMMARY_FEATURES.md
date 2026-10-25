# Solver spine summaries as live predictor features

This connects the **existing solver-generated** `shared-markers.egg` contract to
`structural_runtime`. It is an opt-in experiment in the actual saved raytrace
Pass 3 native event loop, not a new standalone matcher or a saturation proof.
Original queries, RHS, schedule, merge/rebuild and exhaustive CPU completion
remain unchanged. This experiment does not enable the earlier loop-query
CPU takeover or joint sat-skip; their historical credit is not added here.

## What is newly connected

`summary-markers.egg` is an unchanged copy of the solver's four definitions from
`automatic-refutation/egg_layout/experiments/shared_loop_markers/shared-markers.egg`.
The original compiler selected these from 690 offered layouts with measured
prefix work. We reuse its choices and typed binding contracts, not rerun a
solver with invented costs. The four shared prefixes are:

```text
DoWhile(input, body)
  + ContextOf(input, input_context)
  + ContextOf(body, body_context)
  + HasArgType(input, input_type)
```

`summary_features.py` reads the `@marker` contract and checks that its rendered
relation/rule agrees with the `.egg` definition and native profile schemas.
It retains parameter positions and repeated-slot equalities. The current bridge
accepts slot-only, connected typed prefixes. Literal/guard operands are rejected,
not dropped; this is not support for arbitrary marker DSLs.

`summaries.rs` compiles these prefixes into a shared incremental DAG. Native
committed row changes update projected-key arrangements and full correlated
binding tuples. A later stage extends its parent's tuples. Support counts allow
multiple witnesses, removals, function replacement, subsumption and native
canonical row changes after union. Repeated appearances of one table are updated
in order, including self joins. No original rule is executed early.

A marker emits a feature **at each exported parameter position**. The feature
says that the current typed value participates in that position of a complete
joint binding; it is not the arbitrary ID of a Cell or a concrete eclass. The
full binding stays in the view; the model receives only the existential signal.
Thus this version does not encode all numerical parameters or relationships
between two different marker instances into its vectors.

The unchanged base vocabulary has 94 tokens; these four markers add 13 tokens.
The same 256-coordinate encoder and Metal matrix kernel consume them. A model
predicting table `T` cannot receive any summary whose body reads `T`. In
particular, ContextOf/HasArgType summaries are masked for their own targets.
They may inform a different target, such as ExprIsPure. No new marker is allowed
to certify purity or discharge an original condition: that credit remains zero.

## Honest prediction measurement

The normal timing path uses only the augmented model. A diagnostic audit also
runs the unchanged model in shadow on **the same unknown (model,key) conditions
at the same pre-Run state**. It excludes cached exact answers, and compares both
scores to the current exact projected relation. This reports true/false
positives and negatives, separated by whether an eligible summary was present.
No second inference schedule or post-RHS target state supplies these labels.
Shadow queries, oracle reads and audit time are excluded from timing claims.

Training still uses the saved final graph of this same program and the original
held-out-key split. Live requests can include training keys; this is not an
unseen-program generalization measurement. Predictions never determine coverage:
CPU checks the complete complementary candidates as before.

The earlier routing counters mix prediction and exact cached answers. They are
reported separately and never converted into pure GPU accuracy. New summary
maintenance, source receipts and joint work are also reported rather than calling
these features free.

## Validation and reproduction

Focused tests cover joint-binding correlation (different witnesses cannot each
supply half the conditions), late facts, replacement, removal/clear, duplicate
witnesses, repeated-table self joins, randomized updates, and actual native
union followed by subsume. The diagnostic live run independently rebuilds final
marker relations from source receipts and compares all complete binding tuples;
existing native-query audits still check original full incremental bindings.

Use the existing `export.py` arguments plus:

```sh
--summary-markers experiments/structural_runtime/summary-markers.egg \
--base-model out/structural-runtime-build/model.json \
--output out/structural-runtime-build/summary-model.json
```

Run the same `run.py` validation/benchmark adapter with `--coordinates markers`
and `--model out/structural-runtime-build/summary-model.json`. The `audit` lane
performs paired model and native-binding verification; `gpu` times the new path.
Unchanged native/GPU-off controls are reused through the existing explicit flags.
Run the feature-off `model.json` separately with the same binary and worker.
`--extract` invokes the existing bench skill's production Tiger/DAG checks.

Read-only comparison (including fixed-rule-weight check-depth curves):

```sh
python3 experiments/structural_runtime/summary_compare.py \
  --before out/structural-summary-control-serial \
  --after out/structural-summary-features \
  --output experiments/structural_runtime/summary-results.json
```

The aborted `out/structural-summary-control` attempt overlapped the candidate's
Tiger extraction. It was explicitly invalidated and is not used for timing.
The serial replacement is the only accepted new control, not a best-of sample.

## Measured result (one serial run per timed configuration)

Same binary/ordinary GPU worker/input, old base model versus augmented model.
All numbers below come from `summary-results.json` and the preserved run dirs.
The unchanged native and GPU-provider-off controls remain historical.

The paired audit contains **21,773 unknown condition requests**. These exclude
answers already available in the exact cache, not all 25,247 rule anchor visits.

| Paired condition outcome | Unchanged model | Summary features |
|---|---:|---:|
| True positives | 1,644 | 1,669 |
| False positives | 3,940 | 3,930 |
| False negatives, subsequently completed by CPU | 1,832 | 1,807 |
| Recall | 47.30% | 48.01% |
| Precision | 29.44% | 29.81% |

Only **124 requests (0.57%)** have an eligible new summary feature. All 124 are
negative and **both models already reject all of them correctly**. Consequently
there is no observed new correct decision on the actually summary-bearing keys.
The 25 fewer false negatives/10 fewer false positives occur elsewhere after
refitting the model; they are not 35 newly recognized Cell instances. These live
quality figures also must not be confused with earlier frozen-final-graph
held-out accuracy: intermediate native states have different predicate contents.

The four summaries each have **84 final correlated bindings**, sharing four
incremental stages. They process 70,563 row notifications and only 518 joint
binding deltas; their explicit maintenance is **35.56 ms**. They retain 38,404
source receipts and expose features at 245 typed values. Logical entry counts
are not RSS or compression ratios. Their source selection was optimized for
loop-context consumers, while this predictor's seven rules predominantly ask
purity conditions: that workload mismatch limits feature coverage.

| Timed metric | Base features | Summary features |
|---|---:|---:|
| Complete Pass 3 inference | 22.610 s | 18.027 s |
| Provider maintenance + query | 455.71 ms | 423.83 ms |
| Exact condition checks | 30,607 | 30,608 |
| Original complete bindings returned | 3,945 | 3,945 |
| Predictor rows evaluated on GPU | 388 | 392 |

**Do not attribute the large wall-time difference to markers.** There is no
reduction in exact checks, ordinary GPU dispatches (7,259), native row events
(2,458,173), unions (239,476), or ordinary-worker upload/download bytes. Large
parts of the timing difference are in unchanged native RHS (6.210 vs 5.309 s),
rebuild (4.365 vs 4.019 s) and ordinary GPU/IPC phases. This one pair does not
establish a causal end-to-end speedup. No repeated runs or best-time selection
were used to turn it into a favorable claim.

Validation: **20 Rust tests + 2 Python tests**, **12,586 native incremental
full-binding query audits**, final summary joint-binding oracle, exact full graph
(**699,068 original rows**, including functions/equality/visibility), and
production Tiger **29-function selected DAG equality** passed. No original table
was removed from comparison. The previously known fused-native protocol
limitation remains; this is the common-submit control, not pristine parity.

The bridge is useful infrastructure, but this particular four-marker library
has not produced useful search reduction. The next feature selection should be
priced against the *remaining uncertain conditions of the actual predictor*,
not the old loop-context query workload or raw marker instance counts. Keeping
current defaults unchanged avoids making users pay for an unproven extension.
