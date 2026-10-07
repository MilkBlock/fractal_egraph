# Parameterized size calculations and conditions

Cell preparation now exports `origin.analysis_program` for Expr-size and
ListExpr-size. The existing concrete replay still verifies the original history;
the additional representation erases observed analysis values from its template,
not from that evidence. It does not union concrete saturated states or claim that
all concrete ripen output literals have already been removed.

An external field injection becomes its own Input parameter. The parameter is
identified by its source field/injection occurrence, not its numerical value.
The observed value is stored separately in `instance_inputs`. Original rule
constants (thresholds, offsets and tuple indices) remain literal.

The template stores structured `Input`, `Const`, and `Compute` nodes, ordered
stage steps, original guards, before/proposed/after update expressions, and final
fields. Supported operations are +, -, *, min and max; official size fields must
use min merge. Constant arithmetic is folded only when checked i64 arithmetic
succeeds. No overflow-sensitive algebraic reassociation is performed.

Conditions on the SAME symbolic expression are merged into inclusive i64 bounds:

* n > 6 and n > 10 -> n >= 11.
* n > 6 and n < 10 -> 7 <= n <= 9.
* n > 10 and n < 10 -> impossible.

The original ordered guards remain available. A condition after a set refers to
its updated expression, for example min(n,n-1), not automatically to n. Comparisons
between different symbolic expressions and != stay as residual constraints.
`impossible=false` means no contradiction was established by this simple interval
pass; it is not a satisfiability proof.

Unknown scalar conditions/actions retain a structured syntax tree and mark the
program incomplete. Unsupported merge, union requiring key-congruence mapping,
and visibility changes also prevent a complete claim. This representation does
not erase those obligations or authorize optimizing the original runtime.

## Actual eggcc example

Prepared saved record 2628 (the entry behind the previously inspected Cell 1962):

```
p0 = input Expr-size(Get(t,0))  // observed 27
p1 = input Expr-size(t)         // observed 25
Expr-size(Get(t,0)) := min(p0,p1)
```

The exported template contains neither 27 nor 25. They are instance data only.
See `eggcc-size-example.json`. This was a selected saved-entry check, not a new
full analyze run or a Cell coverage benchmark.

## Verification

```sh
cargo test --offline --release --lib native_analyze::ripen::entry
```

26 tests passed, including six scalar representation tests and an integration
case that captures native rules, prepares entries for size 25 and size 27, and
checks identical exported templates with distinct instance values. Other tests
cover two-sided bounds, contradiction, i64 limits, min/arithmetic through stage
changes, independent fields with equal values, and unsupported merge handling.
