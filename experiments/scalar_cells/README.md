# Scalar Cell transfers inside an original Run

This is an intermediate optimization, **not yet a successful speedup**. It
discovers unary i64 `:merge (min old new)` writers with one constructor spine,
keyed scalar ports and checked addition. The actual eggcc program supplies 17
Expr-size/ListExpr-size rules. Nothing is inferred from sampled costs or erased.

The CPU Cell index assembles the exact structural frontier. Native residuals
still bind computed LHS values and execute the original RHS. All other rules
run in the **same native action/merge/rebuild batch**; splitting them into two
commits could change guards or merge callbacks. Ordinary Run boundaries remain.

When checked LHS and RHS calculations succeed, a proposal already dominated by
the current min value is a certified no-op. Same-batch min proposals for one
destination are reduced to their minimum. LHS overflow stays a failed match;
RHS overflow is submitted to native execution, even if the destination already
has a smaller value. These are separate cases in the regression suite.

The original rule-environment guard now has an exact generation stamp rather
than rebuilding/sorting all rule names on every callback. Original rule/ruleset
edits change the stamp; clones preserve it; private query preparation does not.

## Intermediate complete Pass-3 observations

Same-run GPU controls, one release run per lane, complete public graph checks
passed. These are not averages, pristine-upstream results or extraction tests.

| Variant | GPU control without assistance | With current Cell assistance |
|---|---:|---:|
| Initial scalar program | 18.349 s | 22.942 s |
| Linear reads, unchanged-dependency reuse and batch min reduction | 18.388 s | 21.080 s |

The second run assembled 1,921,167 structural scalar candidates, rejected 2,502
LHS arithmetic failures, certified 773,501 old-value min no-ops and 24,864
same-batch dominated proposals, and submitted 1,120,300 native candidates.
Arithmetic/index preparation took 1.417 s and the combined native commits
3.864 s. Other bookkeeping is not included in those two timers.

In 1,328 of 1,798 always-run iterations, the remaining native rules matched
nothing. Those iterations still assembled 1,405,271 scalar candidates. This
motivates a **guarded numerical fixed-point Cell**, rather than treating the
current per-Run implementation as the completed performance objective.

Evidence: `out/raytrace-scalar-cells-v1/report.json` and
`out/raytrace-scalar-cells-v2/report.json`; the reproducible full-schedule entry
is `bench.py run --joint-sat`. Source is
`cell-assisted-egglog/src/parameterized_cell/scalar.rs`. The `scalar_cells` tests
cover finite-Run visibility, union in the same commit, arithmetic failures,
min reduction, stateful consumers and environment invalidation.

## Guarded numeric closure (2026-10-20)

An explicit `saturate work`, optionally preceded by independent helper
saturations, may be represented as a numeric dependency graph after the other
rules in `work` have no logical matches. All rules must be semi-naive; no
remaining rule or helper may read/write the selected fields, including
transitive reads in installed merge callbacks. Finite Runs remain unchanged.
The guard is checked against the installed environment, not just source names.

A visible constructor instance supplies the ports of an equation. Actual row
stamps activate the first wave; a missing port waits; a changed min value wakes
only its consumers. All equations in one wave read the same old vector. Checked
LHS overflow filters a match. A possible RHS error abandons the speculative
calculation before publication, retaining the ordinary native error path.
There is no guessed iteration or candidate limit. Unsupported negative/range
conditions and hidden function rows retain ordinary execution.

**Publication keeps the computed waves in order.** Collapsing all waves into
one final write was rejected: on full Pass 3 it produced one extra reversed
`Smaller` record, despite matching every other public row/value/equivalence.
That failed experiment remains at `out/raytrace-scalar-wave-v1`. It is not
counted as a valid acceleration. The accepted path avoids repeated original
LHS searches but retains the native min writes of each computed wave.

`out/raytrace-scalar-wave-routed/report.json` measures the actual complete saved
Pass-3 schedule, one release execution per lane:

| Lane | Inference |
|---|---:|
| Fused native | 24.152 s |
| Same-submit CPU | 24.479 s |
| GPU, joint assistance off | 18.377 s |
| GPU plus guarded Cell programs | 17.782 s |

Both the same-submit CPU/assisted and GPU-off/assisted strict full public graph
checks pass. The ~1.033x improvement over GPU-off is **small**, not the completed
performance goal. Preparation, graph export and extraction are excluded from
these times. Production Tiger extracted all three public graphs successfully;
the selected DAGs are byte-identical (29 functions, 8,018 selected public rows).
Fused native still has its previously documented different intermediate helper
records and is not the exact-graph speedup denominator.

There were 11 numeric closures: 1,289 local waves evaluated 1,405,424 equations,
with 1,988 LHS arithmetic failures and 19,001 final changed coordinates.
Original `always-run` executions fell from 1,798 to 518. These are numerical
Cell closures, not fresh terms producer-DAG macro hits (those are still zero).
The indexed event dispatcher touches only programs reading the changed table;
reverse value indexes are allocated only for actual output-directed reads.

`scalar_closure` regressions additionally cover missing ports, deleted outputs,
helpers with merge callback dependencies, observers of intermediate values,
shared-runtime integration and failure preservation. `bench.py test` runs both
scalar suites.
