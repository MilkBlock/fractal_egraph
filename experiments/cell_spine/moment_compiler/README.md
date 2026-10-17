# Exact boundary coordinates and a real GPU dataflow

Standalone research prototype, not an egglog engine optimization. Four real
math-microbenchmark rewrites Bake two finite Cell templates. The compiler turns
their root/non-root Mul interfaces into a fixed numerical program. Native egglog
independently checks the resulting bindings. Metal/MPS executes feature creation,
projection, GEMM, distance fusion, and consumer selection on an Apple GPU.

No external Python dependencies or network access are needed. CPU tests require
the repository's native examples to have been built. GPU execution requires macOS,
Metal/MPS, and the installed Swift compiler; no CUDA/Tensor Core claim is made.

## What changed

1. **Input identity uses bits, not a domain-sized orthogonal basis.** A q-bit ID
   becomes q signed binary coordinates. Equality is zero squared distance.
2. **Commutative pairs use first and second moments.** No permutation enumeration
   is needed during encoding or matching. Repeated inputs such as `a+a` work.
3. **Roles use concatenation.** Conjunction is a sum of nonnegative distances;
   one failed condition cannot be compensated by another successful condition.
4. **Shared conditions become shared matrices.** Deduplication uses symbolic
   `(instance, circuit node)` wires, not observed binding values or match results.
5. **A fixed GPU program replaces per-pair condition objects.** It still computes
   pair matrices and pays their quadratic output cost. There is no subquadratic
   Join theorem here.
6. **Union can invalidate the interface, even if encoding remains correct.** The
   experiment exposes a stale-library failure and repairs it by alias-specialized
   Bake, preserving the original observation ports.

## The algebra

Let `x` be the q signed bits of a canonical input ID. Define:

```text
lift(x) = [x_i ; x_i*x_j for i<j]
Pair(a,b) = (lift(a)+lift(b))/2
```

Width is `q(q+1)/2`. Coordinates of `Pair` are in `{-1,0,1}`.
This code is injective on **unordered pairs with multiplicity**.

Proof: from the code recover `s=a+b` and `Q=aaᵀ+bbᵀ`. The missing diagonal of Q
is always 2 because input coordinates are ±1. Then:

```text
2Q - ssᵀ = (a-b)(a-b)ᵀ.
```

An outer product of a real vector determines the vector up to sign, including
the zero case. Consequently `{a,b}` is recovered as `{(s+d)/2,(s-d)/2}` for either
sign of d. No assumption that `a != b` is needed.

For arbitrary child feature vectors, retain the diagonal as well. The same proof
gives an exact unordered-pair representation. The compiler uses this extension
for equal-shaped non-leaf children. Different-shaped children have distinct roles
and are concatenated. By induction the encoding is injective on this finite
binary-tree domain modulo the **certified** child swaps.

The certificate is extracted from an unconditional, unsubsumed `Add` swap rewrite
in egglog's parsed AST. Conditional commutativity does not grant the certificate.
This is not a proof for arbitrary egraph equivalence: associativity, other unions,
and other operators require their own representation/guarantees.

### Equality and conjunction

For feature rows X, compute:

```text
G = X Xᵀ
D_ij = norm_i + norm_j - 2G_ij
match_ij = (D_ij == 0)
```

Unlike the old orthogonal code, distinct IDs need not have dot product zero.
Variable feature norms are retained. For multiple role conditions:

```text
D_total = D_role1 + D_role2 + ...
D_total == 0  iff  every role condition is satisfied.
```

Every component is nonnegative. There is no cancellation between failed guards.
Alternative witnesses remain separate; independently true partial projections
are never mistaken for a simultaneous full binding.

### Finite precision

Exact FP32 integer evaluation is allowed only when `4*width*coordinate_bound²`
is below `2²⁴` for each matrix block. The Metal feature compiler uses safe math.
FP16 input mode also requires integer coordinates to fit its exact integer range;
GEMM writes an FP32 matrix. Every numerical feature, match bit, and consumer output
is checked against the integer reference on the tested M3 Max.

This is a bounded arithmetic contract, not a tolerance test. A port to another
backend must validate its input conversion and multiplication/accumulation mode.
The finite q-bit ID namespace must also be respected; there is no fixed-width
exact encoding of unbounded new identities here.

## Compilation and sharing

`compiler.py` contains a small DAG: `port`, `lift2`, `mean`, `concat`, `moments`.
Common instructions are interned by relative input slots. It compiles the baked
interface terms without inspecting concrete bindings.

Two layouts are emitted:

- **Fused:** concatenate roles and evaluate one matrix per interface shape.
- **Shared:** split concatenated roles, reuse a requested matrix whenever its
  symbolic row set covers another request, then gather and add distances.

There is no search over arbitrary new subsets, representative budget, or learned
matching threshold. This is not a globally optimal layout solver. More matrix
sharing can add launches and gathers; both layouts and costs are retained.

Fixed wiring maps condition rows back to the original Spine channels. One
condition result may serve several different bindings and Add connections. It
does not merge these bindings or invent an outer Add that is absent from the graph.

The actual-context oracle exports an ordinary saturated graph, derives external
Add connections, and supplies those connections to the numerical program. This
adapter is **validation infrastructure**, not a demonstrated cheap production
router. GPU `emit` is a fixed gather over already supplied connections.

## Results on the eight original instances

| Query | Previous Pair marker logical MACs | Fused coordinates | Shared coordinates |
| --- | ---: | ---: | ---: |
| Root | 1,088 | 640 | 640 |
| Root and internal | 8,736 | 4,000 | 3,776 |

The all-position shared plan uses two matrices: `28×4` and `8×10`. They retain
64 original channels; repeated input conditions are computed once. The results
are **462 port-scoped full bindings**, or **110 distinct untagged bindings**, equal
to native egglog when all requested Add contexts are explicitly supplied.

In the unmodified graph with only existing Add contexts, the independent native
comparison finds **14 canonical rule bindings**. On the tagged projections,
236 supplied external connections produce 70 successful tagged connections;
these are not 70 different untagged matches.

Cost accounting includes feature construction (96 products, 80 additions, 80
half scales, 128 input components read, 112 concat copies), feature tape size,
projection components, norm products/additions, matrix cells, distance
terms, and retained channels. Logical MAC counts exclude hardware tile padding,
norm computation, dispatch, memory movement, and output work. They are not time
or RSS compression ratios. The GPU currently rebuilds its feature tape; the
dependency-based incremental updater is validated in the CPU reference.

## Union: a negative result and a bounded repair

With `union(a,b)`, refreshing input codes makes the old plan numerically correct
for its old interfaces, but **incomplete**: it finds 512 of 618 native bindings.

For example, after `a=b`:

```text
x*(a+b) = x*(a+a) = x*a + x*a = a*(x+x).
```

The `Mul(a, Add(x,x))` alternative was unavailable in the distinct-input Bake.
It is not merely a different value for a pre-existing projected slot.

The repair groups affected instances by `(source template, input equality
partition)`, Bakes three alias-specialized interfaces, and recompiles their
projection program. It recovers all 618 native bindings, including on the GPU.
The new library keeps the same observed ports so the oracle comparison has the
same domain. Unchanged templates reuse their original descriptions.

Normal CPU execution and the Metal runner both check the declared input partition
before accepting a plan. The stale-plan number above is obtained only by an
explicit diagnostic bypass; an invalid plan is not silently accepted. The Metal
runner's admission check is on the host, outside the GPU matching/timing interval.
It compares the few slots of each instance, not every pair of Cells. Its cost
and potential caching must be included in a future runtime evaluation.

This demonstrates a **guard -> specialize -> recompile** path. It does not yet
integrate general union invalidation into the production engine. Internal unions
that do not appear as input aliases can require a different specialization.
The number of alias partitions can grow rapidly; this experiment does not
pre-enumerate or claim to bound all such cases.

## Failures deliberately retained

- First moments collide: `{00,11}` and `{01,10}` have the same sum.
- Second moments cannot encode arbitrary bags: the four even-parity vertices
  and four odd-parity vertices of a 3D cube have identical degree-1/2 moments.
- Flattening parentheses would conflate distinct interfaces. Shape and role
  information remains in the compiled layout.
- Recursive exact moment lifting can explode: widths `4 -> 10 -> 65 -> 2210 ->
  2445365`. A large Cell does not require this expansion if its queried interface
  stays small, but a large symmetric queried interface can be expensive.
- Shared layouts need not win: the root case has equal MACs and more shared-plan
  launches. These results do not establish GPU/CPU end-to-end speedup or a fraction
  of neural-network throughput.

## Reproduce

```sh
python3 experiments/cell_spine/moment_compiler/demo.py \
  --output out/moment-next --gpu

# After building the native examples, --skip-build avoids rebuilding.
python3 -m unittest experiments.cell_spine.moment_compiler.test_compiler -v

# One larger synthetic operator batch; no native large-egraph claim.
python3 experiments/cell_spine/moment_compiler/stress.py \
  --bake out/moment-next/bake --gpu-binary out/moment-next/gpu \
  --output out/moment-stress --count 512 --bits 32 --half-inputs
```

Output directories must be new. `demo.py --gpu` runs FP32 and FP16-input/FP32-output
variants, including the union-specialized plan. `program.json` contains the input
matrix and static program. The generated executable and full outputs remain
untracked. `results.json` and `stress-results.json` retain only compact text results.

The 512-instance synthetic case has 4,096 interfaces, two matrices (`1792×32`,
`512×528`), and 241,172,480 logical MACs. Every output is checked against an
independent structural-key oracle, which is not used to group or execute the GPU
program. Timing is a single dispatch, not a throughput study; Swift/JSON setup
and readback are explicitly outside GPU timing.

## What this suggests next

Bake can produce both a semantic interface certificate and its concrete numerical
program. The next unresolved issue is choosing the observed interface and its
validity guard so that transitions change few coordinates and rarely demand a new
specialization. Encoding more of the entire closure is not automatically useful.

The same feature data can in principle feed a learned ranking operator, while an
exact compiled gate remains responsible for accepting a match. Such a model
could order work; skipping its low-scored work would need a separately stated
completeness policy. No neural component is implemented or evaluated here.
