# A coupled polynomial Cell: AC and distribution

2026-10-09. Reproduce:

```sh
python3 experiments/cell_polynomial/multirule.py
```

This standalone experiment validates a specialized closure description against
actual native egglog saturation; it does not replace the matcher or discover the
abstraction automatically. Binary/source hashes are in multirule-results.json.

## Rules and input

Four unchanged rules from egglog/tests/math-microbenchmark.egg:

```egg
(rewrite (Add a b) (Add b a))
(rewrite (Add a (Add b c)) (Add (Add a b) c))
(rewrite (Mul a b) (Mul b a))
(rewrite (Mul a (Add b c)) (Add (Mul a b) (Mul a c)))
```

Input: x*(a0+...+a(n-1)), with all variables distinct. After saturation, graft
one more summand into the same native graph and saturate again. A fifth-rule
ablation also enables the original reverse factoring rule:

```egg
(rewrite (Add (Mul a b) (Mul a c)) (Mul a (Add b c)))
```

It leaves the exact closure unchanged on this input family. This is a bounded
observed property of the tested family, not general rule redundancy.

## Why the previous interval representation fails

Commutativity admits arbitrary subsets, e.g. a0+a2, not just contiguous intervals.
The correct coordinates here are nonempty subsets S of distinct summands:

* U[S]: their ordinary sum.
* V[S]: x times that sum, including partially/fully distributed representations.
* One additional class for x.

The two families are coupled. Their node grammars are:

    U[{i}] = Var(ai)
    U[S] = Add(U[A], U[B]) for every ordered nonempty disjoint split S=A union B
    V[S] = Mul(x,U[S]) | Mul(U[S],x)
           | Add(V[A],V[B]) for the same splits

No declaration asserts two arbitrary arithmetic expressions equal: the native
rules establish exactly these tested equivalence classes. The symbolic subset
key is provided by the experiment, not inferred from rule traces.

## Non-01 polynomial recurrence

Let f(S), g(S) count distinct expression trees in U[S], V[S], respectively:

    f({i}) = 1
    f(S) = sum_(A disjoint B = S) f(A)*f(B)  for |S|>1
    g(S) = 2*f(S) + sum_(A disjoint B = S) g(A)*g(B)

The 2 is multiplication orientation, not an arbitrary weight. Products combine
independent child choices, and sums enumerate distinct root constructors/splits.
These are expression counts, NOT numbers of rewrite histories, rule firings or
committed mutations. There are no cyclic child dependencies in this grammar.

For labelled leaves and exponential generating functions, the coefficients are
f_n/n! and g_n/n!, respectively. They satisfy:

    F(z) = z + F(z)^2
    G(z) = 2*F(z) + G(z)^2

Equivalently F=(1-sqrt(1-4z))/2 and G=(1-sqrt(1-8F))/2, choosing the formal
series solutions with zero constant term. No numerical convergence is assumed.
The script verifies the coupled-series coefficients against subset enumeration
for n=1..8. Scalar series count structures; the subset grammar is still needed
to preserve concrete binding identities. The count alone is not a Cell.

## Native verification

| Summands n | Add e-nodes | Mul e-nodes | All Math e-nodes | e-classes | Root expression trees |
|---:|---:|---:|---:|---:|---:|
| 3 | 24 | 14 | 42 | 15 | 168 |
| 4 | 100 | 30 | 135 | 31 | 3792 |
| 5 | 360 | 62 | 428 | 63 | 119520 |

Root expression counts are computed from the verified acyclic grammar, not from
individually extracting all trees. Native checks cover every predicted Add split,
every Mul orientation, and pairwise inequivalence of every predicted class. Add
and Mul table sizes are checked both before and after those assertions, and final
Math node/class counts are checked. This validates the entire described finite
closure, not just the root equality.

Run matrix: starting n=3 and n=4; combined saturation and alternating saturated
AC / one distribution round; four rules and five rules. Eight native runs, two
stages each, all passed and agreed on the structural results. Alternate schedules
are run to saturation, not compared at equal finite rounds.

## What changed from the one-rule experiment

A single interval template no longer suffices. Yet multiple rules do not force
one template per rule or per rule-history string: TWO mutually related families
represent all the tested structures. The resulting representation is a coupled
polynomial grammar rather than a single linear recurrence or low-rank matrix.

Counts still grow exponentially: there are 2*(2^n-1)+1 e-classes and
2*(3^n-2^(n+1)+1) Add nodes. Storing every subset/split explicitly is expensive.
A parameterized descriptor can encode the family, but querying/materializing
arbitrary members still costs work. Native egglog already shares expression
trees; the table is not evidence of new compression or acceleration.

Limits: distinct leaves, one common multiplier, no other arithmetic rules,
no guards/containers/update semantics, no extraction on implicit storage,
no automatic grammar mining, no GPU/SVD, no measured production speedup.
