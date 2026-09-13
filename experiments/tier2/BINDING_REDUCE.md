# Multi-apply-point binding contracts

`rules/reduce.egg` includes `rules/binding_reduce.egg`. The entry is:

```text
LayerReductionInput(fractal, dag, environment)
  → LayerEndpointView(fractal, ReduceDag(dag, environment))
  → BResult(outputs, requirements, effects)
```

Run `higher` followed by `endpoint-reduce`. `CompleteBinding(result)` certifies
that a representative with fully substituted output, requirement and effect
vectors exists. It does not assert that guards hold, recursion terminates, or an
analytic closed form exists. Residual Substitute/ParameterAt nodes cannot alone
establish completion. The JSON status is `substituted`, not `reduced`.

## Recursive interfaces

`Definition::Recur { name, callee, output, arguments }` becomes
`ERecur(callee, output, arguments)`. The callee and output port are part of
identity; occurrence position is separate evidence. Equal pure calls can share
one node while multiple uses remain multiple references. Different callees or
outputs cannot accidentally share merely because both use local position zero.

The native adapter follows actual `UnitReturn` candidates into the next entry's
input ports. Each return argument must resolve to this layer's outputs through
an actual parent binding. Missing external inputs produce `needs_boundary`, not
a fabricated recursive call. Only witnessed returns are emitted. It exposes the
next entry's output ports of the callee's ordered local output interface.

Callees are registered by exact definition equality in `NativeBindingCallee`,
including source rules, datatype, input/output roles and algebra interpretation.
Short graph-local identities reference this registry; no hash collision is used
as evidence of equality. Source roles use AST paths, not physical source offsets,
so history replay preserves the definition. `ObservedCalleeLayer` connects each
callee to its observed layer contract, not an unconditional unfolding rule.

## Binding DAG and contracts

`compile_contract` uses egglog's expression parser. Variables resolve only to
inputs or preceding definitions. Names disappear from the expression DAG.
Independent definition schedules serialize identically when their ordered
output/requirement/effect interfaces are identical. A rooted iterative postorder
assigns definition positions; slot offsets are computed on demand without
shifting all existing bindings. Pure identical expressions are interned and dead
pure definitions are omitted. Ordered ports, repeated uses and alias distinctions
are preserved. This is structural normalization, not arbitrary graph isomorphism
or condition implication. Runtime environment lookup still has its own cost.

Immutable `BindingExpr` syntax and reducible `EndpointExpr` results are separate
sorts in the same egglog. Algebra cannot feed expanded result terms back into
syntax substitution. Physical rows use explicit `ERow` fields; `EColumn` projects
those fields without assuming constructor injectivity or undoing unions.

Read witnesses and source equality/predicate conditions enter the requirement
vector. Only captured committed writes/unions enter the effect vector, carrying
original identities alongside symbolic payloads. These are descriptions, never
executed actions. Equality conditions are not global egraph unions. Conditions
and effects keep ordered roots; the compiler does not merge condition sets.

## Mathematical interpretation

The default `integer-safe` mode recognizes typed i64 `+`, `-`, `*` as EIAdd,
EISub, EIMul. It applies neutral/absorbing identities valid for integer operands;
it does not turn bounded machine arithmetic into unbounded polynomial arithmetic.
Each source primitive also keeps a structured definedness obligation, so removing
an outer expression cannot silently discard an inner overflow precondition.

`EGG_LAYOUT_BINDING_ALGEBRA=math` explicitly interprets source Add/Sub/Mul/Const
as the existing exact scalar endpoint algebra. Other source operators remain
opaque. This is a declared interpretation of the source DSL, not inferred just
from constructor names. Use the same setting for history replay. Division,
floating-point identities and overflow invariants are not inferred.

## Actual runtime reproduction

```sh
EGG_LAYOUT_DISCOVER_RECURSION=1 cargo run --release -- analyze \
  --recapture-tier0 --source experiments/recursive_patterns/integer_binding.egg \
  --output out/binding-contract-integer

EGG_LAYOUT_DISCOVER_RECURSION=1 EGG_LAYOUT_BINDING_ALGEBRA=math \
  cargo run --release -- analyze --recapture-tier0 \
  --source experiments/recursive_patterns/math_binding.egg \
  --output out/binding-contract-math

cargo test --test binding_reduce --test tier2_reduce --test recursive_patterns
```

Each recursive instance in `recursive_patterns.json` has `binding_reduction`:
callee identity/definition, recursive return bindings, local output addresses,
extracted endpoint expressions, requirements/effects and the replayable tier2
input fragment (requires that run's imported FractalComb context). That fragment
is not a standalone tier0 program.

The integer fixture actually generates two branches, `2*n+0` and `2*n+1`.
Tests check extracted native outputs `2*n` and `2*n+1`. The Math fixture checks
that actual `Add(x,Const(0))` outputs simplify after capture. Binary/ternary
fixtures check recursive return linkage, contracts and exact history replay.

These are finite witnessed recurrence interfaces. General multi-parent/coarse
family discovery, proving an arbitrary-depth recurrence, solving its closed
form, and proving dissipation are still distinct tasks. No original tier0 facts
are removed, and these tests make no compression-rate or speedup claim.
