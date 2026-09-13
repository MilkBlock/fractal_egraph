# Multi-apply-point binding reduction

`rules/reduce.egg` now includes `rules/binding_reduce.egg`.
`LayerReductionInput(fractal, dag, environment)` produces
`LayerEndpointView(fractal, ReduceDag(dag, environment))` in `higher`.
Then run `endpoint-reduce` to obtain `BResult(outputs, effects)`.
This is **one layer**, not reduction of the entire observed extent or a proof for
arbitrary depth. Depth is never used as the number of applies.

`src/binding_reduce.rs::compile` uses egglog's expression parser. Inputs and
preceding definitions are available by name; names become local parameter slots.
`BLet` extends a persistent environment. Repeated references do not duplicate
definitions in the encoded program. Forward references, redefinitions and reused
recursive site IDs are rejected. Reuse a definition to express a shared call.

Example interface:

```text
inputs: x, extra
shift = EAdd(x, 0)
left  = recur(site=0, args=[shift])
right = recur(site=1, args=[EMul(extra, 0)])
twice = EAdd(left, left)
outputs = [twice, right]
effects = [union(left, right)]
```

With inputs `[7, 9]`, the result is `[EAdd(left,left), right]`, where
`left = ERecur(0,[7])` and `right = ERecur(1,[0])`. The union remains an opaque
effect expression; it does not merge the two endpoints. An ERecur is a symbolic
boundary, not an instruction to create future tier-0 facts.

Immutable `BindingExpr` syntax and reducible `EndpointExpr` results are separate
sorts in the same native egglog. Otherwise algebraic identities can feed expanded
terms back into substitution and cause unbounded work. Existing endpoint algebra
is reused only for explicitly named EAdd/ESub/EMul/EDiv/EPow expressions. Other
calls remain opaque, and integer literals do not imply machine-overflow semantics.

## Actual capture integration

With recursive template discovery enabled, each recursive instance now contains
`binding_reduction` in `recursive_patterns.json`. The native adapter lowers the
entry apply and its observed branch applies, including physical row projections,
into one ordered binding DAG. Parent output ports connect actual dependencies.
All output addresses are retained; multiple outputs need not be combined by Add.
Successful lowering runs both native rulesets and checks a BResult exists.
Unsupported lowering reports its reason instead of supplying a made-up summary.

```sh
EGG_LAYOUT_DISCOVER_RECURSION=1 cargo run --release -- analyze \
  --recapture-tier0 --source experiments/recursive_patterns/binary.egg \
  --output out/binding-reduce-binary
cargo test --test binding_reduce --test tier2_reduce --test recursive_patterns
```

The adapter preserves source calls as opaque `source::operator` expressions.
It does **not** infer their mathematical meaning from names, infer a closed form,
or establish recursion invariants. Row/union mutation evidence and keyed-read
requirements remain in the original instance certificates. The pure value
interface is not a replacement for these certificates. The scalar Reduce tests
and explicit multi-site algebra tests exercise mathematical reduction; the real
binary/ternary capture tests exercise automatic dependency wiring and native
evaluation, not a new compression-rate or runtime-speedup claim.
