# Integral rules: a compact generator with branching binding growth

2026-10-09. Reproduce with:

```sh
python3 experiments/cell_polynomial/integral.py
```

Ten bounded native runs verify depths 0..4, with/without Mul commutativity.
This is an exact symbolic family prototype, not a saturated Cell runtime,
not a numerical integration algorithm, and not a speedup benchmark.

## Actual rules and starting facts

Three rules copied from math-microbenchmark.egg (all are active with comm enabled):

```egg
(rewrite (Integral (Add f g) x) (Add (Integral f x) (Integral g x)))
(rewrite (Integral (Mul a b) x)
 (Sub (Mul a (Integral b x))
      (Integral (Mul (Diff x a) (Integral b x)) x)))
(rewrite (Mul a b) (Mul b a))
```

Seed Integral(Add(Mul(f,g),Mul(h,g)),x), with four distinct opaque variables.
First saturate integral linearity. For each requested depth, saturate multiplication
orientation and run one IBP round. Finally saturate orientation and collect State.
Do NOT saturate IBP: this selected system keeps making new opaque Diff/Integral
terms. Fixed depth is an explicit experimental scope, not a claim of convergence.
Each depth run starts a fresh engine; within the run, all rounds share the graph.

## Exact binding generator

Let D(a)=Diff(x,a), I(b)=Integral(b,x), C(a,b)=Integral(Mul(a,b),x).
Then the actual rule states:

    C(a,b) = Sub(Mul(a,I(b)), C(D(a),I(b)))

Without commutativity the next binding pair is (D(a),I(b)).
With commutativity a second direction is available: (D(b),I(a)).
Pairs are normalized modulo the swap because their Mul nodes are equivalent.
The generator preserves unary words (base variable, word over D/I), never
collapsing them merely by their length or number of D/I symbols. D(I(a)) and
I(D(a)) are distinct in this selected rule system. No cancellation theorem or
unstated calculus identity is injected.

The two initial branches share the integral chain of g in the no-comm case.
The script uses immutable shared term descriptions, not a dense binding matrix.
The finite generator is small while the generated binding universe grows.

## Native observations

| IBP depth | Fresh normalized states, no comm | Fresh normalized states, with comm | Total states with comm | Ordered State tuples with comm |
|---:|---:|---:|---:|---:|
| 0 | 2 | 2 | 2 | 4 |
| 1 | 2 | 4 | 6 | 12 |
| 2 | 2 | 8 | 14 | 28 |
| 3 | 2 | 16 | 30 | 60 |
| 4 | 2 | 32 | 62 | 124 |

These are sets of distinct binding states, not execution/event counts. At depth4,
without comm there are 10 total normalized pairs and 15 distinct unary binding
terms; with comm, 62 pairs and 93 unary terms. The selected system permits
arbitrarily long words; the numeric doubling table itself is verified only at
the listed depths, not extrapolated as a measured infinite result.

For every run, the script checks every predicted State tuple and its exact
cardinality, every expanded binding's IBP equality, and pairwise inequivalence
of all distinct unary terms. State/Add/Mul/Sub/Diff/Integral/Var sizes are checked
before and after assertions so checks cannot silently create missing nodes.
Native source and binary hashes are saved in integral-results.json. Generated
.egg files and logs are in out/cell-polynomial-integral.

## Consequence for matrix / series modeling

The useful object is a noncommutative binding generator with constructors and
an effect-producing clause. It can be viewed as two symbolic shift-and-swap
operators on pairs of unary words. For a finite truncation it has a sparse
matrix representation; adding depth introduces new coordinates. Its formal
iteration series describes paths, but paths, normalized binding states and
committed facts are three different counts.

The C recurrence also preserves Sub/Mul effects. Counting continuation states
alone is not enough to reconstruct the complete egraph. This experiment verifies
those effects natively but does not implement a compressed effect executor.
The small generator does not guarantee a small fixed-dimensional exact matrix,
low rank, or cheap enumeration of all distinct bindings/results.

This complements the AC/distributivity example: some interacting rules admit
subset-indexed polynomial families, while IBP plus swapping needs word-indexed
recursive binding families. A general system would need to infer and validate
such typed generators; this script supplies the generator manually.

Limits: opaque f/g/h, fixed integration variable, no trig simplifications,
no general Diff algebra or inverse laws, no arbitrary unions/set/subsume,
no automatic grammar discovery, no production acceleration claim. In particular
mathematical polynomial differentiation terminating does not imply this selected
syntactic rewrite system terminates: its required simplification rules are absent.
