# Nonrecursive eggcc queries as projected binding factors

This is a **manual representation exploration**, not a new parameterized Cell
family, a production backend, or a speed benchmark. It uses real saved raytrace
Pass-3 tables and source queries. The inputs are arbitrary typed identities,
multi-valued relations and integer positions; no affine recurrence, repeated
loop body, unique context, or stable Cell instance is assumed.

The full source groups are in eggcc `ae80f82`:

* `dag_in_context/src/optimizations/loop_strength_reduction.egg`: the five-atom
  DoWhile / ContextOf / Const / HasArgType producer of `lsr-inv`.
* `optimizations/non_weakly_linear.egg` and `optimizations/passthrough.egg`:
  the shared If passthrough LHS.
* `utility/terms.egg`: the three Smaller cost comparisons.

Historical hotspot evidence is in the existing
`experiments/pass3_boundary_programs/results.json` in the assisted worktree.
The loop query accounted for 75% of candidate visits **in that old profile with
its existing boundary provider disabled**. That is not its share in the current
optimized engine, not total inference time, and not this prototype's speedup.

## 1. A full binding tensor need not be built

For each actual DoWhile enode row r, retain:

```
L[r,input_context]     ContextOf(inputs,...)
H[r,input_type]        HasArgType(inputs,...)
O[r,output_context]    ContextOf(body,...)
C[constant_row,ctx]    the context of this existing Const row
```

These are column projections, scatter and indexed reads of raw current tables,
not a precomputed list of complete rule matches. Raw IDs are compacted into
typed axes during input encoding; that preprocessing and its memory are NOT
free and are not timed here.

Compute `Q = O C^T`. The nonnegative factored tensor

```
B[r,ic,it,const] = L[r,ic] H[r,it] Q[r,const]
```

represents all bindings relevant to the RHS, without allocating B. An output
decoder may enumerate it when native effects must be committed. That unavoidable
output work is not eliminated. Multiple contexts/types remain independent
choices exactly as in the original LHS; no singleton lookup assumption is used.

RHS Const construction needs only `(constant_value,input_type,input_context)`.
With a raw Const-row-to-value projection V, and
`G[r,(ic,it)] = L[r,ic] H[r,it]`, compute

```
NewConst = G^T (support(Q) V) > 0.
```

The actual snapshot has 84 DoWhile rows, 1,543 Const rows and 36 constant-value
identities. The 122,846 distinct projected RHS coordinates need only 1,440
distinct new Const keys. Native hashcons already stores equal constructors once;
these are opportunities to share construction/lookup WORK, not new node-storage
compression. All lsr-inv effects still have to be represented or emitted.

### A signed dense basis

For a separator alphabet padded to K, a Walsh matrix satisfies `H H^T=K I`.
Store `X=O H` and `Y=C H` and recover `Q=X Y^T/K`. This handles arbitrary sets,
not just two individual identity codes. Full joint separator keys must be kept
when several variables are correlated.

Source row insertions/deletions are signed row/code updates. A supplied canonical
key merge M acts as `X' = X H^T M H / K`; integer tests cover this operation.
This only updates a supplied key domain: it is NOT a general union-find,
congruence rebuild or discovery of newly enabled constructor witnesses.

**Changing basis does not lower rank or prove performance.** In this snapshot
there are only two context keys. Encoded inputs are 100% nonzero and compatible
root/Const pairs are 94.78% occupied, but the product has inner dimension TWO.
It may be better to retain two outer products than launch a large GEMM. Do not
call that density a demonstration of neural-network-like arithmetic intensity.
A separate 64-key, multi-port fixture verifies that the circuit is not hardcoded
to two keys, with no claim about its prevalence in production.

## 2. Iteration can retain factors

For current live rows, split each factor into below-watermark and fresh parts.
Do not subtract already-thresholded result sets: a fresh witness may produce an
already-known RHS key and would be incorrectly lost.

Ignoring the root ownership factor for notation, use the exact identity:

```
L H Q - L_old H_old Q_old
  = (L-L_old) H Q
  + L_old (H-H_old) Q
  + L_old H_old (Q-Q_old).
```

The root adds one more disjoint term. `Q-Q_old` itself is one signed contraction:
`[X,X_old] [Y,-Y_old]^T/K`. Four factor terms carry the incremental query instead
of expanding a full binding tensor. Tests compare this with exhaustive witness
ownership for 100 random fresh-row selections. A native query using explicit
Fresh predicates agrees for 67,720 projected coordinates in a synthetic
watermark over the real snapshot. **These are not captured production cursor
timestamps and do not establish a drop-in semi-naive runtime.**

The `LoopClosure` experiment goes one step further: with the background
DoWhile/ContextOf/HasArgType relations fixed, keep Const existence in
`S[(context,type),value]`. The source rule only copies these fields from finite
input domains, so this carrier is closed under this ONE rule. Build
`W=G^T O`, then iterate

```
U[context,value] = max_type S[context,type,value]
S_next = max(S, support(W U)).
```

No full binding expansion is needed to feed newly constructed Const keys into
the next round. This is checked for five rounds against a scalar implementation
on the multi-port fixture. It is not yet a native/GPU whole-rule feedback test;
the native and GPU checks below validate the projected query and RHS-key circuit.
Changing background relations requires committing/retaining earlier deferred
effects, not simply replacing W and discarding history.

## 3. Two attempts that must not be sold as dense GEMM successes

### If: keep correlation, but do not preallocate every coordinate

The correct shared signature is `(input_position, arg_type)` jointly, attached
to the Get output class. Old Arg contexts can be existentially projected away
for these two consumers; the joint position/type cannot. A regression with
`{(4,A),(9,B)}` versus `{(4,B),(9,A)}` rejects false matches from independent
marginals, and checks a late witness and a union-enabled match natively.

The initial rectangular representation has shape `4382 x 23 x 74`: 7,458,164
positions with only 797 nonzeros. It is a bad dense layout. The second version
first maintains `AnyArgGet[value]`, then masks potential If frames. All 88
remaining coarse frames in this final snapshot have at least one empty side;
no fine signature matrix needs allocating. The exact native query also returns
zero. This is a compound-view absence test, NOT Cell proof credit, and not
evidence that every intermediate production round has zero matches.

Fine joint coordinates are built only for surviving frames. This improves the
representation, but presently leaves gather/scatter, masks and small set
intersections; it does not demonstrate a broad Tensor Core workload.

### Smaller: use the RHS quotient, retain guard semantics

The RHS unions with an existing endpoint eclass, so term witnesses inside that
class do not change the target. Existential `<` can use min(left)<max(right);
`>` can use max(left)>min(right). Equality alone requires a cost-set intersection,
not overlapping bounds. When the three same-ruleset union-only rules are viewed
together, the left-target condition is min(left)<=max(right). Multi-cost tests
retain both possible targets and equality holes.

Native rule-specific targets agree on 73,371 saved Smaller rows. But all 63,038
TCPair classes in this snapshot are singleton. The practical representation is
mostly reductions and comparisons; the experiment does not claim dense GEMM
benefit or new saturation authority for this case.

## Validation and limits

`results.json` records the saved snapshot hash, native/worker binaries, full and
fresh-row query checks, and the losing layouts. Original constructors omitted
from an isolated query become opaque typed leaves. Native query replay verifies
the complete projected RHS coordinate set; it does NOT commit the real rewrite
RHS or prove equality of complete Pass-3 graphs/extraction.

The Metal worker receives factors, computes shared matches and RHS constructor
keys with MPS GEMM and masks, and agrees for 15 full/delta jobs, including a
64-key unstructured fixture. Dot products have checked integer bounds below
2^24 for exact FP32 accumulation. It is Apple MPS, not a demonstrated NVIDIA
Tensor Core backend. Payload identities stay outside low-precision arithmetic.

Input encoding, dictionaries, raw-table gathering, CPU output expansion and
native update journals are still separate prototype work. The worker processes
prepared jobs; this is NOT a resident production event loop. No timings or 90%
offload claim are made. A complete optimization must include those costs and
preserve current visibility, rule-stage ownership and non-idempotent effects.

## Reproduce

Use Python with NumPy and the existing `native_math_baseline` binary. Outputs
must be new directories; build products and native inputs remain under out/.

```sh
python -m unittest discover -s experiments/separator_context_operator -p 'test_*.py' -v
/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
  -O -sdk /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
  experiments/separator_context_operator/gpu.swift -o out/separator-worker
python experiments/separator_context_operator/run.py \
  --snapshot PATH/graphs/cpu.control.json --native PATH/native_math_baseline \
  --gpu out/separator-worker --output out/separator-checked
```

The first failed native attempt used forward datatype declarations; the two
interrupted attempts exposed a pathological output regex. The successful
oracle now parses anchored lines without nested digit quantifiers. No failed
attempt is presented as a verified run.
