# One program for composition, observation and updates

2026-10-09. This change focuses on representation and interaction, not performance.
No benchmark was run for this revision. No egglog engine source was modified.

## Core representation

`program.py` is the small reusable core:

    typed, shared operator DAG
        + named cell ports (references only)
        + one input environment/cache per instance

Cells are names for parts of the program, not separate storage containers. Naming
or overlapping cells does not copy nodes or allocate another instance cache.
Composition connects existing node IDs through typed operators. A new consumer
can read an existing internal port without changing prior nodes or cell names.
This is structural interning of operator expressions, not automatic discovery of
semantic equivalence between arbitrary programs.

```python
program, outputs = cicm_program()
instance = program.instance(input_values)
instance.observe(outputs)

# New external consumer of an already-defined internal expression.
inner = program.port("size", "bopLeft")
consumer = program.lt(inner, program.const(8))
instance.observe({"small_internal": consumer})

# Atomic boundary update: one evaluator derives all affected changes.
instance.patch({"a": 4, "a_alias": 4})
instance.observe({"small_internal": consumer})
```

`observe` walks only missing dependencies and stops at cached boundaries. It does
not traverse a whole old cell merely to discover that all values are cached.
`patch` first validates and installs the entire input batch, then recomputes cached
dependents in topological order. If an operator's value does not change, propagation
stops on that edge. Unobserved branches remain unevaluated. This handles arbitrary
pure supported operator composition without a separate handwritten updater per
Cell. It is an exact change propagator, not an analytic derivative approximation.

Supported scalar operators: integer add/scale/min, equality/order, Boolean and,
and integer mask conditions. The interpreter uses exact signed Python integers;
bools are distinct. CPU/Metal lowering has a stricter nonnegative uint32 domain,
checks intermediate bounds, and rejects unsupported shapes rather than wrapping.

## Existing compiler now consumes the same Program

`lowering.py` builds the source-checked CICM size/selector program using this core.
`run.py` no longer maintains a separate handwritten Metal computation and CPU
formula. The CPU body, Metal body, interpreter and update mechanism all consume
this Program. `backend.swift` is an execution/ABI template; compiling it directly
without generating a CPU body gives an explicit error for CPU execution.

Observation roots also define the code-generation slice. Verification adds an
internal consumer after the original program is built, then generates:

1. The original six-output CPU/Metal program.
2. A seven-output program including the new observation.
3. A one-output observer-only program containing just its six needed DAG nodes.

The observer-only program excludes the right-size and original selector operators.
Prior nodes remain byte-for-byte unchanged. The current native submission ABI is
still the original six-output size/selector interface; the extra observer output
is a verified observation, not a new committed egglog transformation.

## Deep interaction test (not a native egraph-size claim)

Two independent 64-stage chains use s[i+1]=min(100,2*s[i]+2). The shared DAG has
388 nodes, 640 edges, dependency depth192, and128 cell aliases. Only the two final
roots are initially observed. Add a consumer of left.stage48.size afterward:

| Case | New operator evaluations | DAG nodes visited | Untouched regions |
|---|---:|---:|---|
| Existing cached instance | 1 | 3 | All old computation |
| Cold instance | 147 | 148 | Last16 left stages and all64 right stages |
| Boundary patch blocked by unchanged first min | 3 recomputations | See report | Later stages and other branch |

The warm instance retains389 scalar cache entries after observing the new node;
the cold instance retains149 including input bindings. These are explicit storage
costs, not free interactions. A fused GPU execution retains only its output buffers;
it must compute the needed slice if a later observer requests an unretained value.
The cold test models that distinction. No assertion is made that all time/memory
tradeoffs disappear or that the illustrative chains are actual discovered Cells.

## Correctness checks

```sh
python3 experiments/cell_kernel/test_program.py
cargo build --offline --release --example cell_kernel_oracle  # if missing
python3 experiments/cell_kernel/verify_shared.py --gpu
```

The second script generates and compiles the shared-program backends. It requires
Swift and Metal on macOS; omit --gpu for CPU plus native verification. In restricted
sandboxes actual GPU access may need authorization. It does not run benchmarks or
produce performance tables. Low-level execution logs may contain tool timings,
which are not used as performance claims.

Twelve unit tests cover independent reference evaluation, simultaneous updates,
type rejection, late/new inputs, warm/cold observation, traversal stopping,
min propagation barriers, unchanged inputs, aliases and independent branches.

The integration test checks32 synthetic instances against the independent scalar
reference, original source LHS/size rules, generated CPU code, and actual Apple
Metal execution. Both normal and newly observed outputs agree. Original size and
selection results are submitted through the unchanged native oracle and all
committed result rows/full bindings match. Seven live-instance patch sequences
agree with fresh evaluation. Results: program-checks.json and shared-verification.json.

## Boundaries not solved by this change

* Internal ports are known explicitly. Arbitrary pattern search inside compressed
  structures, or deriving the port from a new coarse-rule match, is not implemented.
* This is the fixed-shape size/selection compilation cut from the prior pilot,
  not a complete egraph Cell evaluator, automatic bake or full CICM RHS execution.
* Equality IDs can be supplied as canonical boundary inputs. General union,
  congruence closure, subsume and concurrent mutations are not implemented here.
* All evaluated scalar values remain in the interpreter's cache; no automatic
  eviction policy or optimal fusion/caching planner is claimed.
* Updates are processed sequentially. Operator effects are exported by the outer
  native-commit adapter; arbitrary mutation effects cannot be reordered as if pure.

The engineering property established is narrower and useful: larger logical Cell
names do not force opaque containers, duplicated state, whole-Cell evaluation for
every observation, or a second handwritten update implementation.
