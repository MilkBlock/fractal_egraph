# Exact polynomial binding prototype

Measured 2026-10-09. This is a **standalone algebra prototype**, not a new
storage backend or accelerated egglog matcher. The native oracle uses the cached
`native_math_baseline` binary; its hash and source rule hash are in results.json.

## Run

```sh
python3 experiments/cell_polynomial/verify.py
# Override --binary if the cached oracle is elsewhere.
# Missing oracle: cargo build --release --example native_math_baseline
```

No Python dependencies. The command generates five runnable .egg files under
out/cell-polynomial, executes all five, and verifies 648 union-update prefixes
against an independent explicit enumeration. The .egg files check all 20 possible
Hit tuples positively or negatively and check each predicted associativity RHS.
A failed native check or algebra equality aborts the program. `example.egg` is the
first nontrivial union case, directly runnable by native_math_baseline.

## Math microbenchmark subset

Exactly one original rewrite from egglog/tests/math-microbenchmark.egg:

```egg
(rewrite (Add a (Add b c)) (Add (Add a b) c))
```

We construct four interface keys k0..k3. Each key is unioned with a distinct
inner Add(b_j,c_j). Outer Add(a_i,k) facts supply five outer variables; a2 has
both k0 and k1 as right children. All a_i, b_j, c_j names are disjoint, so these
fixtures have no accidental nested-Add matches. A Hit relation exposes the
same LHS for checking. Each native phase starts a fresh engine with its chosen
interface partition and runs **one** rule round, not saturation. Consecutive
algebra phases reuse the factor representation; native phases independently
reconstruct their inputs, rather than feeding previous RHS facts onward.

P[i,k] records outer Add(a_i,k). Q[k,j] records inner Add(b_j,c_j) at k.
E[k,l] records current equivalence. J = P E Q counts raw witnesses. Its positive
support, not its integer counts, is compared with canonical native Hit facts.

For disjoint classes A,B, an effective union has the exact update:

    delta J = (P u_A)(u_B^T Q) + (P u_B)(u_A^T Q)

No J entry is enumerated by Model.union(). The verifier deliberately expands J
for equality checking. Afterward, historical terms are compacted into the exact
class-block decomposition:

    J = sum_C (P u_C)(u_C^T Q)

Compaction in this prototype recomputes projections; it is not an optimized
incremental implementation. Vector indexing is fixed to this finite fixture.

## Results

| Phase | Union | Raw witnesses | Distinct bindings | Newly enabled | Factor scalar slots | Dense slots |
|---|---|---:|---:|---:|---:|---:|
| 0 | none | 6 | 6 | 0 | 36 | 20 |
| 1 | k0=k1 | 10 | 8 | 2 | 27 | 20 |
| 2 | k1=k2 | 16 | 13 | 5 | 18 | 20 |
| 3 | k0=k0 (redundant) | 16 | 13 | 0 | 18 | 20 |
| 4 | k2=k3 | 24 | 20 | 7 | 9 | 20 |

The final non-01 matrix is exactly one outer product:

    [1, 1, 2, 1, 1]^T [1, 1, 1, 1]

The coefficient 2 comes from a2's two historical outer facts, which canonicalize
together after union. This demonstrates why counts are not e-node counts.
Early factors cost MORE scalar slots than a dense matrix. These counts exclude
P/Q, class membership, identifiers, object overhead and validation copies;
there is no RSS compression claim.

A separate structured 64x64 algebra fixture (not the native oracle) starts with
no matches and gains 4096 distinct bindings on union. One outer product stores
128 scalar slots rather than 4096 dense entries. This is a favorable Cartesian
product case, not evidence of typical math/eggcc workload compression.

## What this proves, and does not prove

Proves on finite cases: union rank-at-most-two update; redundant union no-op;
exact class-block compaction; support agrees with actual one-step egglog rewrite
matching/results on all five small phases. 648 additional union-prefix checks
are algebra-only, not 648 native runs.

Not implemented: SVD, multi-rule saturation, dynamic fact insertion, output
feedback, arbitrary congruence-rebuild cascades, arbitrary guards, set/subsume,
new interface dimensions, Cell discovery from trace, or runtime acceleration.
The experiment verifies a boundary-response factorization, not a fully baked
RipSatCell execution engine. Emitting all distinct RHS nodes can still require
work proportional to the expanded result size.
