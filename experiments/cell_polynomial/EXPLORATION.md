# Two exact follow-up experiments

Date: 2026-10-09. Reproduce with:

```sh
python3 experiments/cell_polynomial/explore.py
```

This is a standalone research prototype. Native checks use the cached egglog
binary whose SHA-256 is recorded in exploration-results.json. No GPU, SVD or
production matcher changes are involved. The reference mathematics uses exact
Python integers and rational Gaussian elimination, with no numerical tolerance.

## 1. Real recursive feedback: associativity as an interval grammar

Take just the original Add associativity rule and a right-associated sum of n
pairwise distinct variables. Unlike the preceding one-step experiment, run native
egglog to saturation, allowing RHS nodes and equality changes to feed new matches.
No commutativity, constant folding or other rule is loaded.

The observed states agree with the interval representation:

    G[i,i+1] = leaf i
    G[i,j] = sum over i<k<j of z[i,k,j] * G[i,k] * G[k,j]

z tags an Add split and the product constructs its ordered left/right children;
this is a tree-grammar circuit, NOT commutative identification of different trees.
Evaluating all z at 1 counts distinct parenthesizations (not rewrite histories).
Counts follow C_0=1, C_m=sum_{k=0}^{m-1} C_k C_{m-1-k}.
The corresponding formal series satisfies C(z)=1+z*C(z)^2.

| Leaves | Root parenthesizations | Native Add e-nodes | Native Math e-classes |
|---:|---:|---:|---:|
| 3 | 2 | 4 | 6 |
| 4 | 5 | 10 | 10 |
| 5 | 14 | 20 | 15 |
| 6 | 42 | 35 | 21 |
| 7 | 132 | 56 | 28 |
| 12 | 58786 | 286 | 78 |

For n<=7, all root parenthesizations are explicitly checked equal in native
execution. For every n, every interval split is checked equal to its interval's
right-comb representative, with Add counts unchanged before/after checks.
Observed counts are n*(n+1)/2 e-classes and binomial(n+1,3) Add e-nodes;
including leaves gives binomial(n+1,3)+n total Math e-nodes.
For n=12, the 58786 count comes from the exact grammar recurrence, not 58786
independent native checks. Native validation covers all 286 interval splits.

This reveals a useful shape for parameterized cells: interval endpoints and split
positions. HOWEVER egglog already shares these trees through its e-classes. These
numbers are not an additional compression or speedup delivered by our prototype.
A future engine could represent the interval-split family implicitly; this script
does not integrate that representation, discover the grammar automatically, or
prove the same representation survives adding arbitrary arithmetic rules.

## 2. Full rank does not imply expensive execution

Let u_i=i+1, v_j=j+2. The non-01 matrix A=u*v^T has rank 1. Apply the guard j<=i:

    T[i,j] = u_i * v_j * [j<=i]
    T = diag(u) * L * diag(v)

L is the lower triangular matrix of ones. T has exact full rank n, since its
diagonal is nonzero. An exact low-rank SVD cannot compress its rank below n.
Nevertheless applying T to a vector takes just:

    prefix = 0
    for i in order:
        prefix += v[i] * x[i]
        y[i] = u[i] * prefix

For n=64: 4096 dense entries vs 128 numeric coefficients plus the known scan
operator; application needs 128 multiplications and 63 additions. This is scalar
slot and operation accounting, NOT RSS or measured execution speed.
The 64 standard basis vectors and a signed integer vector all agree exactly with
dense multiplication. Ranks are checked by rational elimination for n=4,16,64.

More directly connected to formal power series, if S is the one-step downward
shift, then S^n=0 and:

    L = I + S + ... + S^(n-1) = (I-S)^(-1)

The inverse is a difference operator with only 2n-1 nonzero entries. The script
verifies L*(I-S)=I exactly. Thus the guard can turn a rank-one matrix into full
rank while preserving a cheap structured operator representation.

Limits: this is a deliberately ordered guard example, not a measured eggcc rule
plan. Sorting arbitrary keys, changing equivalence classes, materializing all
outputs, and maintaining dynamic inputs have costs not included here. Efficient
matrix-vector aggregation alone does not imply efficient enumeration of every
matching binding or generation of every distinct RHS e-node.

## Consequence for the research direction

Search for shared operator programs, not only low rank: outer products, indexed
interval recurrences, scans, differences, permutations and their compositions.
Keep exact binding/condition semantics alongside numeric analysis. The current
experiments validate two particular representations; they do not establish an
automatic abstraction-search algorithm or production acceleration.
