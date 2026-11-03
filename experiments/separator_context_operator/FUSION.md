# Next unit: a rule group sharing state, not one matrix per rule

One concrete fusion is now represented in `fused_comparator_matrix`: the three
Smaller `<`, `>`, `=` rules share an endpoint-cost state and a single UNION-target
readout. Arbitrary i64 costs are mapped to exact ordinal coordinates; the per-row
state is `[minLeft,maxLeft,minRight,maxRight,valid]`. One 5x2 operator produces
`[maxRight-minLeft+valid, maxLeft-minRight]`, and `>0` selects the two endpoint
targets. Empty endpoints have an all-zero row. This is checked on all 73,371
saved Smaller rows against the previously native-verified union-target reference,
including separate synthetic multi-cost and i64-extreme regressions. It is a
CPU matrix check; the GPU path and runtime frontier admission are not integrated.
The original costs remain payload data: ranks must NOT be used for addition.

This demonstrates actual rule fusion but is a very skinny GEMM, not a Tensor
Core speed result. The next objective is sharing context and intermediate work
across larger producer/consumer groups, rather than padding this matrix.

This is a design boundary for the next experiment, not an implemented fused
eggcc pass. The existing source producer and consumer are in
`optimizations/loop_strength_reduction.egg`. Both are present in the real Pass-3
schedule: the consumer is included through `all-optimizations`, so it must not
be dismissed as dead simply because its name is absent from individual Runs.

The producer creates `lsr-inv(loop, input_constant, output_constant)`. The
consumer reads this relation twice, once for the increment and once for the
multiplied constant. It additionally matches an Add and a Mul sharing an Arg/Get
position. Materializing every invariant and then treating both reads as new
joins loses the producer's information.

## Fuse by retaining the shared relation factors

Keep one background context/type matrix G, one output-context compatibility
factor Q, and delayed Const construction recipes. Both consumer ports refer to
these SAME factors. Existing Add/Mul occurrences restrict which output-constant
coordinates they request. They do not independently rebuild the complete
invariant relation or its Cartesian square.

The proposed large state blocks are:

1. Shared typed/context/Arg/Get observations and their validity masks.
2. Correlated partial bindings for consumer ports, including rule-stage and
   witness identity. Fine coordinates are enabled only when coarser masks allow.
3. Shared RHS construction recipes and distinct action frames for final effects.

Matrix contraction should carry information BETWEEN these blocks and serve
several rules simultaneously. Block-diagonally concatenating independently
compiled rules is not the desired fusion. Neither is computing cross-rule
products that are subsequently thrown away just to make a GEMM larger.

## Conditions that fusion must preserve

* The two `lsr-inv` occurrences have independent input-constant choices. With
  multi-valued input context/type relations they must NOT be forced to choose
  the same one. Likewise a consumer's body context is not automatically the
  producer witness's context; only the actual variable equalities couple them.
* The producer's `always-run` and the consumer's `loop-strength-reduction` have
  distinct scheduling boundaries. A fused physical program must retain stage
  gates; a merely derivable future invariant is not an already published fact.
* The consumer uses `unstable-fresh!`, union and subsume. Shared construction
  work does not authorize merging distinct native action frames or reusing one
  fresh assumption across them. Final native compiled binding identity and the
  effect protocol must govern those frames.
* External seeds or other producers of an intermediate relation must be included
  as additional input factors. Source inlining is unsound if it silently assumes
  that only the chosen producer can supply it.
* Raw table updates, canonicalization and visibility changes still supply the
  authority. There is no inferred fixed-point permission for ordinary Runs.

The immediate question is whether compiling this producer/consumer group
removes an intermediate relation's materialization and repeated search while
preserving these obligations. The current prototype establishes factor and
projection algebra, not that end-to-end result.
