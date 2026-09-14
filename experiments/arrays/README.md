# Tier2 logical arrays

This is native egglog DSL execution, not a Rust mock evaluator. It extends the
existing endpoint/binding DSL; it does not modify eggcc or execute GPU kernels.

## What was borrowed from eggcc

Reference checkout: `~/Repos/egg_related/eggcc`, commit `acc0ae3a`.
Inspected sources:

- `dag_in_context/src/schema.egg`: Arg/Get, tuples, PointerT/StateT,
  Load/Write/PtrAdd/Alloc and DoWhile.
- `dag_in_context/src/utility/subst.egg`: substitution scopes and delayed unions
  to avoid substitution observing its own results.
- `dag_in_context/src/optimizations/mem_simple.egg`: state-dependent memory
  operations and the conditions needed to commute accesses.

The implementation keeps explicit argument slots and separates immutable binding
syntax from reducible values. Logical arrays are pure values, not pointers:
forgetting StateT on a mutable load is NOT a valid way to import an InputArray.
Memory placement, mutable views, alias analysis and materialization are deferred
until lowering. No eggcc memory rewrite was copied as an unconditional array law.

## First-class operations

`rules/arrays.egg` is included by `rules/tier2.egg`.

| Representation | Meaning |
|---|---|
| FiniteDomain(n), StreamDomain | A finite index domain or a lazy infinite sequence |
| InputArray(id, domain) | Immutable named input sequence; id identifies its value |
| FillArray, RampArray | Constant and affine element generators |
| Tabulate(domain, body, env) | Lazy index-to-element function; index is BParam(0) |
| MapArray, ZipArray | Pointwise pure scalar operations |
| SliceArray, ConcatArray | Logical ranges and concatenation |
| ArrayGet(array, index_expr) | Symbolic indexing; no string-encoded index expression |
| ArrayAt(array, i64) | Concrete demand-driven access |
| EArrayValue(array) | Pass an array through the existing binding argument carrier |
| BArrayGet, BArrayGetValue | Capture an array or read one supplied as an argument |
| FoldArray | Ordered left fold with explicit initial value |
| ReduceArray | Exact scalar sum or maximum, with their known identities |
| Blocks, BlockAt, FlattenBlocks | Lazy partition, including non-divisible tail blocks |
| FoldMap, FoldZip | Fused logical reduction alternatives |

This first version handles one-dimensional exact scalar arrays, not arbitrary
rank tensors or a general mixed-value type inference system. Floating-point
reassociation is not justified by these scalar laws.

Input parameters and captured environments remain separate. Tabulate binds the
index at slot 0; ScalarLambda binds the element at slot 0; FoldLambda binds the
accumulator at slot 0 and element at slot 1. Captures follow those arguments.
A BRecur expression can serve as the element generator, retaining its callee,
output port and parameterized binding. This represents a supplied output sequence;
it does not prove that an arbitrary recurrence admits random access.

## Laws and evaluation boundaries

- Map composition and map/fold fusion preserve the function environments.
- Zip requires matching domains. Scalar sum/max can be repartitioned into blocks.
- Arbitrary FoldLambda is NOT assumed associative. For example, subtraction
  remains an ordered fold even after map/fold fusion.
- Symbolic constant/ramp sums call the existing Reduce/AddConstant/AddArithmetic
  rules. They do not generate N elements. Map(exp, Fill(N,0)) sums to N.
- Concrete slices and reads require valid bounds. Symbolic reads require an
  explicit ArrayIndexProof for that array/index; unknown bounds stay unresolved.
  Such proof facts are assumptions in the current analysis context, not a
  general condition solver or permission to transfer guards to another context.
- Streams can be indexed, sliced and partitioned. Whole-stream sums are not
  evaluated or assumed convergent. Only finite slices can be reduced.
- EvaluateFold(array,budget) explicitly enables bounded concrete reference
  evaluation. No default rule requests every element or unfolds every prefix.
- TryBlock(array,size) proposes a block alternative; zero/negative sizes are
  rejected by guards. Symbolic block counts use ECeilDiv.

The small node costs favor fusion and closed scalar forms. They are NOT HBM/SRAM
or GPU performance costs. E-graph alternatives are retained; fewer displayed
operators are not a measured reduction in physical allocations.

## Connection to FractalComb

`rules/higher.egg` accepts explicit ArrayReductionSummary and ArrayFoldSummary
contracts and produces EndpointView expressions that use this array DSL. These
are contracts supplied by a caller or future Bake verifier. Merely observing a
Depth count does not establish an array's domain, element function or fold law.
The tests include this bridge, but no automatic Bake producer is claimed.

## Reproduce

From the repository root, using a new output path:

```sh
cargo run --release -- arrays experiments/arrays/basic.egg out/arrays.json
cargo test --test arrays
```

The example checks symbolic range sums, exp/sum fusion and a block alternative.
The report records extraction results, element-query count and fold-prefix count.
The example has zero element queries and zero expanded fold prefixes.

Tests also cover finite lengths 0/1/5/8/17, several block sizes, tails, invalid
bounds, empty max, zip length mismatch, closure scopes, array-valued binding
arguments, non-associative fold order and bounded stream slices.

Still missing for the full proposed workflow: multi-example FractalRule Bake,
automatic dissipative/array-summary inference, the joint softmax summary algebra,
hardware memory costs and tier0 feedback. This change supplies the array/fold
foundation; it is not a FlashAttention search result.
