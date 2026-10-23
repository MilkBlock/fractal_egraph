# Pass-3 check after the parameterized-marker merge

**Parameterized query reuse helps in this single pair. The automatic layout
chosen by the current model makes it substantially slower.** These are separate
results. This is the actual assisted runtime on the saved full raytrace Pass-3
input, not a toy, pristine-egglog speedup, or generated-raytracer runtime.

All three timed lanes use the same newly built Rust executable, fixed Metal
worker, original schedule, stable selection policy and existing joint proofs.
Each lane runs once. Unchanged CPU/native graph controls are reused from
`out/pass3-compact-layout`; no averages or fastest-run selection. Setup/Bake,
solver, diagnostic counters, serialization and extraction are outside inference.

| Lane | Inference | Compared with preceding row |
|---|---:|---:|
| Existing loop-context provider | 14.129014 s | — |
| Add the seven tuple/If parameterized programs | 12.765186 s | 9.65% lower |
| Select anchors/payloads automatically for these eight queries | 17.459500 s | 36.77% higher |

The first two lanes use `experiments/pass3_parameterized_reuse/baseline.json`
and `combined.json`. The third uses the ordinary scheduled solver's static cost
model over that same eight-query family, including the loop query. Thus the
third comparison tests the whole layout choice, not just numerical payloads.
No program family or original rule is dropped.

Earlier branch results (13.944485 -> 12.720856 s) are historical, not controls
for a same-binary claim here. The new pair observes the same direction but one
run does not establish stable gain or explain cross-version wall-clock changes.

## What is actually faster

Separate diagnostic runs give the same work figures as the earlier branch:
GPU live candidate-row visits fall from **10,405,823 to 7,193,043**. Exactly seven
queries move off the GPU. The 305 common measured GPU rules have identical
fixed-rule-weight depth curves. Queries moved entirely off GPU have no remaining
seed population; their conditional depth probabilities are undefined, not a
100% refutation rate.

The new CPU query providers pay 129,746 additional root evaluations, 98,911 new
index reads and 58,587 non-root candidate visits. These are not free and cannot
be subtracted directly from GPU row visits to manufacture a net operation count.
Their 55,373 complete bindings remain original native RHS executions. These
programs do not instantiate the entire Add-All-Gets recursive closure at once.

## Why automatic layout loses

The GPU performs exactly the same **7,193,043** candidate visits with and without
automatic layout; its common-rule depth curves and every rule's emitted binding
count are unchanged. The regression is principally in CPU query search:

| CPU boundary-query work | Fixed parameterized plans | Automatic layout |
|---|---:|---:|
| Root evaluations | 130,928 | 220,994 |
| Index reads | 100,257 | 17,012,483 |
| Non-root candidate visits | 184,907 | 29,810,100 |
| Whole Cell query service | 0.737646 s | 4.099966 s |

The service grows by 3.362320 s, about 71.6% of the 4.694314 s total regression.
Other phase timings also vary; do not attribute all their changes to one Cell.
This is a stage decomposition, not per-Cell counterfactual credit.

The two If queries explain most of it:

| Query | Fixed root | Selected root | CPU candidate visits, fixed -> selected |
|---|---|---|---:|
| non-weakly-linear If passthrough | If | HasType | 3,255 -> 4,173,413 |
| state-edge-passthrough | If | HasType | 6,121 -> 25,461,043 |

They still produce **0 and 4** bindings. Initial `HasType` has zero rows; the
cost model uses `max(1, rows)` and treats it as a cheap starting table. During
inference it grows to **19,160 rows**. `If` grows only **136 -> 172**. Similarly,
the loop query switches from DoWhile to initially empty HasArgType; actual root
evaluations grow **1,182 -> 34,152**, despite its 123,254 bindings staying equal.

There is a second model defect: private-plan search is priced mainly by root
count times the sum of read costs. It does not propagate conditional branch
fanout through the remaining joins. Shared-prefix storage, meanwhile, is priced
using products of whole-table sizes. This simultaneously undervalues a bad
private root and overvalues many useful deep shared prefixes. The MIP solver
optimizes that model correctly; its optimality is not execution optimality.

The selected numeric marker is also too shallow to save structural joins:
three Add-Gets consumers share its four root-row fields, including `n`, with
**47,509 cache hits** and **18,641 retained bindings**. But it covers only one
atom; no consumer skips Concat/Single or the length lookup. All selected plans
have `prefix_len=0` and `payload_resumes=0`. A high marker hit count therefore
must not receive credit for removed joins. Marker refresh itself totals only
0.007304 s here; storing the integer parameter is not the main slowdown.

## Full-input compiler failure found and repaired

Before running the automatic lane, the unmodified merged compiler failed:
SCIP rejected an infinite objective coefficient. Products of whole-table fanout
bounds exceeded SCIP's finite coefficient range.

The repair uses an exact nonnegative-cost argument, not a candidate budget:
choose a feasible set of private plans, conservatively charging indexes even
when shared, to obtain upper bound **30,518**. Any option whose own mandatory
charges already exceed that complete bound cannot occur in an optimum. Global
shared-marker charges are computed before elimination and kept unchanged;
rational sums of the solver's floating coefficients preserve ties. Sharing
between consumers remains legal.

This eliminates **5,762 of 9,112** layout choices before model construction.
It does not alter the remaining objective, truncate the closure or restrict
rule matching. It makes the model solvable; it does **not** repair its bad
cardinality/selectivity assumptions. That is why the resulting runtime can
still regress. The original failure is retained under
`out/param-marker-pass3-selection`; the evaluated successful plan is under
`out/param-marker-pass3-bounded-selection`.

## Correctness and attribution

- All timed lanes and separate diagnostics pass exact original rows, values,
  equality and visibility against the same-submit CPU: **699,068 rows /
  524,631 mapped e-classes**.
- Both parameterized and auto-layout GPU outputs produce the same selected
  DAGs as the CPU for **all 29 functions**, through the production Tiger.
- Original fused-native versus same-submit graph/extraction disagreement
  remains. Extraction intentionally returns nonzero for that older gate;
  `cpu-gpu-quality.json` passes. No pristine parity claim is made.
- The selected-plan native-frontier audit checks all **41,361** Cell-supplied
  queries without a mismatch. It is separate from final-graph/extraction checks
  and is not a timing run.
- All lanes keep 6,890 Run boundaries, 2,425,040 row events and 239,476 union
  events. Existing joint proof consumption remains unchanged. General query
  reuse does not become a new saturation proof.
- See `extension.json` and `layouts.json` for skill-generated work and depth
  comparisons, and `results.json` for fingerprints, phases and verification.

## Next changes, in order

1. **Fix plan pricing before enabling auto selection.** Use phase-conditioned
   root populations and update frequency, plus actual prefix cardinality and
   per-key fanout. An initially empty analysis table is unknown future work,
   not evidence of a cheap root. Price private and shared plans with the same
   branch-work model. Keep existing known-good plans as explicit alternatives.
2. **Optimize the shared join prefix, not the already located root row.** Retain
   the correlated If branch/index spine or other repeatedly read suffixes, and
   charge their refreshes/subscriptions. Evaluate their net saved reads and time,
   rather than marker cache hit rate. The present query model should not force
   all parameters into every marker.
3. **Then use stronger parameterized transition programs.** For Add-All-Gets,
   propagate changed length/position through a bound instance instead of finding
   the same length again at each step. Preserve one-step finite Run semantics;
   batch a complete interval only within an appropriate saturation proof.
   Native effects and observable intermediate facts still need to be committed.

We have not implemented these three runtime/model improvements in this check,
changed the default execution path, or hidden the losing automatic lane.

## Reproduction

Use `skills/cell-assisted-egglog-bench/scripts/bench.py`. All commands and binary,
worker and input fingerprints are preserved in each lane's `workflow.json`.
The new `boundary-credit --layouts` mode compares the same query family while
allowing its roots and payloads to change. It preserves the original whole-graph,
proof, submission, emitted-binding and diagnostic-provenance gates.

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py boundary-credit \
  --layouts --before out/param-marker-merged-combined \
  --after out/param-marker-merged-selected \
  --profile-before out/param-marker-merged-combined-profile \
  --profile-after out/param-marker-merged-selected-profile --output out/new-layouts.json
```

Use `--extension` with the baseline/combined directories for the added programs.
Inference was built with `DEVELOPER_DIR=/Library/Developer/CommandLineTools`;
all runs share the fixed `commit-marker-worker/matrix`. CPU controls and existing
native/CPU extraction files are reused, not retimed. This report does not include
solver time in its inference column.
