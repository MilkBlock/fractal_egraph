# Recurrent matrix execution of a real eggcc closure

**A fixed-wiring GPU context eliminates the intermediate joins and table writes
for this rule family. GEMM is not the fastest implementation of that context.**
This is a standalone executor for an admitted rule class, not a replacement for
the entire GPU-owned backend or the full Raytrace Pass-3 schedule.

## Actual workload and source

The source is the saved real Raytrace `pass-3-flat.egg`, SHA-256
`c727cdf724b020ae585c4051fda36b66576c1ce4252a4d9d59fbb7522d0c7d4f`.
The unchanged native parser/typechecker exports typed rules. After replaying the
first 1,563 original Runs and checking their update flags, the graph has 276,778
rows. The experiment replaces the next complete `is-resolved` saturation.

All **17 original rules** are admitted, including Function, Const, Top, Bop,
Uop, Get, Concat, Single, Switch, If, DoWhile, Arg, Call, Empty, Cons, Nil and Alloc.
There are 18,674 predicate coordinates and 28,115 distinct grounded clauses.
Grounding reads 28,160 immutable constructor anchors, retaining aliases and all
alternatives in an eclass. It does not search the changing predicate tables or
run the expected closure to construct inputs.

The current admissible class has one immutable structural anchor per rule,
unary existence premises with anchor-bound keys, and unary existence outputs.
Predicate result identities cannot escape into effects. Structural growth,
union, function updates, primitive guards, multiple structural anchors and
hidden output predicates are rejected. This is an explicit specialization of a
rule family, not a claim that arbitrary eggcc rules meet those conditions.

## What the matrix actually does

The state is a GPU-resident presence vector indexed by typed predicate and bound
input slot. An If clause reads four existing coordinates: resolved predicate,
input, then branch and else branch. It writes the If output's resolved coordinate.

Ridge regression fits the conjunction's Boolean truth domain. Analytic sufficient
statistics avoid enumerating 2^k training examples. Positive rescaling allows
FP16 coefficient storage. For the four-premise case, the actual learned score is:

```
score = -1 + 0.818359375 * (pred + input + then + else)
enabled = score > 1.8642578125
```

The model is checked as an exact separator: the all-true score must exceed the
threshold, and the largest false score must stay below it, with an FP32 error
margin. Positive coefficients make these two checks cover every Boolean input.
The worker revalidates the FP16 coefficients and Boolean initial state. A bad
model is rejected before iteration; this is not approximate prediction with a
second LHS search. Ridge is a compilation method here, not evidence that learning
is necessary: the original conjunction is already known.

Each iteration performs:

```
fixed slot gathers -> FP16/FP32 simdgroup GEMM -> threshold
    -> OR into next state -> next iteration
```

Eight independent clauses occupy one matrix row and eight result columns. All
non-padding output columns are used. The weights are block diagonal, with only
12.5% nonzero entries; zero multiplications are still real overhead. Matrix
execution is actual Metal matrix arithmetic, not a scalar predictor called GEMM,
but no high occupancy or neural-network throughput claim follows.

New predicate facts remain in the state vector. There are no general table
writes, hash joins, per-candidate exact fallback or CPU binding transfers in the
iteration loop. Birth timestamps reconstruct all visible Run snapshots from one
final readback. CPU controls command batches and receives a small convergence
status; it does not decide individual transitions. The fixed-point bound follows
from the finite monotone lattice, not a heuristic timeout.

## Verification

The GPU reaches its own fixed point before the native reference supplies closure
results. All **532 Run boundaries**, including the final quiet Run, agree with
native predicate facts and update flags. There are **18,674 new facts**. The final
full typed graph comparison passes: **295,452 rows and 229,455 mapped classes**,
including unchanged functions, equality and visibility. GPU results are decoded
once after convergence and are never repaired with native rule execution.

The small fixture also covers an unseeded cycle, aliased inputs, multiple
representations of one class, and disjunctive support. Seven compiler tests cover
Boolean corners and rejection boundaries, and a corrupted separator is rejected
by the real worker. This does not certify in-loop external union or graph growth.

## Timings and the necessary control

One run per lane, M3 Max. The same worker, program, input, bindings, schedule
frontiers and exact publication semantics are used for the matrix/direct pair.
The unchanged native timing control is reused. No averages or fastest repeats.

| Selected closure only | Host-controlled execution | Device time |
|---|---:|---:|
| Native CPU, no intervening exports | 326.2 ms | — |
| Learned separator + GEMM recurrence | 63.5 ms | 35.1 ms |
| Same fixed wiring, direct GPU conjunction | **42.2 ms** | **16.9 ms** |

Thus the matrix phase is 5.13x faster than this native CPU phase, while the direct
GPU phase is 7.73x faster. Both GPU birth-time vectors are byte-identical to the
native-checked result. The direct control proves that the benefit is not uniquely
due to regression or GEMM: it also eliminates dynamic joins and materialization.

Do not present these as end-to-end or full Pass-3 speedups:

- One-time anchor/wiring compilation costs **173.5 ms**.
- Native prefix replay and snapshot acquisition are preparation, about 4 s here.
- The measured initial matrix subprocess includes 144.2 ms of worker startup,
  compilation/upload and execution; this is separate from the 63.5 ms phase.
- Full typed row encoding for the final verification costs **1.445 s**, because
  it reconstructs the entire original graph. It is not a measured production
  incremental commit of the new predicate rows. That handoff remains unconnected.
- Native control cloning costs 14.4 ms, excluded from its closure timer.
- The 1.54 MB allocated GPU buffers cover this selected context only, not the
  complete egraph, CPU graph or library-preparation memory.

The matrix loop executes about 557 million padded FLOPs, including zero products,
and revisits clauses on successive rounds. Native logical matches total 28,115;
those are not native candidate-row counts. No search-work ratio is inferred from
dividing these incompatible quantities.

## Reproduce

Use Python with NumPy. The existing `out/cell-spine-venv/bin/python` is sufficient.
Build artifacts, binary frames and native logs remain in `out/`.

```
python3 experiments/matrix_closure/build.py --output out/matrix-build --target TARGET
python-with-numpy experiments/matrix_closure/run.py \
  --source PASS3.egg --frontend out/matrix-build/oracle/frontend \
  --worker out/matrix-build/worker --output out/matrix-checked \
  --ruleset is-resolved --control SAVED_DEVICE_PASS3_RUN
python3 experiments/matrix_closure/compare_worker.py \
  --reference out/matrix-checked --worker out/matrix-build/worker \
  --output out/matrix-vs-direct
```

For the small fixture, use `fixture.egg --ruleset closure` and omit `--control`.
Use `test_compiler.py` for the finite-domain tests. `results.json` preserves the
verified scopes and measurements. Automatic loading into the main backend and
replacement of a production schedule node are not implemented by this experiment.
