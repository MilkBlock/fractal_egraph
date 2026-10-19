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
