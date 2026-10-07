# Parameterized size calculations and conditions

Cell preparation first validates the concrete historical replay, then lowers
supported parameterized size entries into the actual executable local `.egg`.
`origin.analysis_program` retains the structured program and instance data;
`origin.symbolic_lowering` identifies the executable mode. Unsupported cases
retain the concrete program with an explicit reason, rather than erasing guards.

The executable mode changes size-function output sorts to `RipenSize`, rewrites
arithmetic into RSAdd/RSSub/RSMul/RSMin/RSMax nodes, and propagates symbolic types
to size-derived function outputs (including eggcc's to-hoist-size). It replaces
both external injections AND concretized historical RHS assignments using the
symbolic step program. Concrete scalar receipt checks have already run before
lowering and are replaced, not used to constrain the symbolic graph to training
values. Other concrete replay checks remain.

Recorded path conditions become symbolic lower/upper bounds and residual
predicates. Original scalar guards query symbolic predicates; known interval
implications and constant comparisons can enable them. Unproved guards remain
deferred: this is a conditional symbolic closure, NOT full saturation over all
possible numeric inputs. It does not automatically union different concrete
Cell states or claim equivalent unguarded execution. Existing scalar signatures
are still required to be complete; union/key-congruence and unsupported operations
are not silently generalized.

Symbolic programs initialize their own typed environment, bypassing the concrete
compiled-environment and parent-fork caches. This is currently a correctness-first
path; no initialization speedup is claimed.

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

29 tests passed, including six scalar representation tests and an integration
case that captures native rules, prepares entries for size 25 and size 27, and
checks identical exported templates with distinct instance values. Other tests
cover two-sided bounds, contradiction, i64 limits, min/arithmetic through stage
changes, independent fields with equal values, and unsupported merge handling.

## Executed on the real saved Cell

Record 2628 was prepared and ripened using the generated symbolic `.egg`. It
finished; the resulting state contains RSParam and RSMin, and no 25/27 scalar
literals. See `executable-result.json`. The local full program and state are in
`out/ripen-symbolic-size-demo/entry.egg` and `state.json`. The original 27/25
remain in the instance evidence, not in the size values of that executable graph.

Additional execution tests verify native parsing and execution, symbolic RHS
addition, two-sided interval guards and failure of an unproved guard to fire.
