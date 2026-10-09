# An exact, compositional quotient of integration-by-parts histories

2026-10-09. Run:

```sh
python3 experiments/cell_polynomial/operator_quotient.py
```

This experiment changes the semantic domain deliberately. It uses polynomial
functions over Q, D=d/dx, and I f(x)=integral from 0 to x of f(t) dt. It is NOT
the existing egglog experiment with opaque Diff/Integral constructors. The
existing math microbenchmark's integral simplifications cannot silently be
reinterpreted as anchored integration (e.g. integral_0^x sin(t) dt=1-cos(x)).
No egglog execution or runtime acceleration is claimed here.

## Operator laws and effect

DI=1, ID=1-E0, where E0 evaluates at 0 and returns the corresponding constant
polynomial. Thus D and I do not commute; the defect is the rank-one boundary
projector E0.

On a binding pair f tensor g, two IBP directions induce

    T_minus = D tensor I
    T_plus  = I tensor D.

Commutativity of the product allows choosing which factor to differentiate.
Keep the original factor roles ordered; swapping to apply the rule and swapping
back yields T_plus. Integration by parts, with C(f,g)=I(fg), gives

    C(f,g) = f*I(g) - C(D(f),I(g))
           = g*I(f) - C(I(f),D(g)).

In particular the signed sum of boundary products along a history of length n is

    A_w = C(f,g) - (-1)^n C(T_w(f tensor g)).

This is an identity of polynomial functions, not a prescription to solve an
integral by recursively referring to itself. A summary of the endpoint and
parity determines the accumulated boundary expression's denotation, but not the
list or syntactic shapes of its intermediate e-nodes.

## Three-integer normal form

Read a history as steps -1/+1. Let d be final displacement, a the negative of
the minimum prefix displacement, and b the maximum prefix displacement, including
initial displacement 0. Then exactly:

    T_w = (I^(a+d) D^a) tensor (I^(b-d) D^b).

Proof: the empty word has (0,0,0). On appending a step, cancel adjacent DI=1 on
each factor. Uncancelled ID remains and records its boundary condition. The
normal form updates to the prefix extrema above. This proves the formula by
induction; it does not assume ID=1.

On ordinary monomials x^k tensor x^l this is zero if k<a or l<b. Otherwise it is

    [k! l! / ((k+d)! (l-d)!)] * x^(k+d) tensor x^(l-d).

The coefficient is genuinely non-01. Different triples define different operators
on the full polynomial tensor space: d determines output degrees, and a,b are
the minimum input degrees admitting a nonzero result. Monomial action proves the
operator identity on arbitrary polynomial tensor inputs by linear extension.
Evaluation of an arbitrary SET of polynomial outputs still requires preserving
the set; summing output vectors is not a membership-preserving replacement.

## Composition, powers and boundary effects

Execute (d1,a1,b1), then (d2,a2,b2):

    d = d1+d2
    a = max(a1, a2-d1)
    b = max(b1, b2+d1).

Composition is constant-size arithmetic (not constant bit complexity for
unbounded integers). At depth n there are O(n^3) possible triples, compared with
2^n histories. A set of triples deduplicates residual OPERATORS exactly.
A composed pair of sets can be computed by pairwise composition and dedup; it is
not claimed that arbitrary set composition is constant time.

For n>=1, the nth power is

    (n*d, a+(n-1)*max(-d,0), b+(n-1)*max(d,0)).

For d=0 this gives T^2=T: balanced histories are boundary projectors. For nonzero
d, required input degree grows linearly under repetition. For finite polynomial
inputs that eventually kills the result, yielding a precise dissipative condition.
This interpretation is specific to polynomial degree, not every effect in egglog.
If total degree is at most M, any triple with a+b>M is the zero operator; the
number of nonzero summaries is O(M^3), independent of history length.

The commutator is a boundary term:

    T_minus T_plus - T_plus T_minus = E0 tensor 1 - 1 tensor E0.

The script also verifies it on total-degree-four monomial tensors using two
5x5 matrices over exact rational numbers. Those matrices act within a fixed
TOTAL-degree space, so no invalid degree clipping is used.

## Measured exact checks

* 65,472 word/input comparisons, all words of length 0..9 on 64 monomial pairs.
* 65,472 accumulated IBP-effect checks against the telescoping identity.
* 3,969 concatenation/summary composition checks.
* Associativity of all triples of the reachable summaries from histories <=5.
* Closed-form powers and balanced-history idempotence on those summaries.
* Growth to depth40 computed by expanding/deduplicating summaries, not by
  enumerating 2^40 histories. Correctness of this dynamic program follows from
  the composition law above; exhaustive word comparison is only through depth9.

| Exact depth | Histories | Distinct residual operators |
|---:|---:|---:|
| 8 | 256 | 54 |
| 12 | 4096 | 139 |
| 20 | 1048576 | 505 |
| 32 | 4294967296 | 1784 |
| 40 | 1099511627776 | 3310 |

These are representation counts, not a measured memory/speedup comparison.

## Interpretation and limits

The research target is to quotient composed rewrites by an operator algebra,
retaining boundary defects instead of treating every relative-binding word as
uninterpreted. The DI relation is a familiar one-sided-inverse algebra; no claim
of discovering a new algebra or literature novelty is made. The proposed use is
an explicit Cell/binding summary with exact composition and power operations.

This preserves functional residual binding semantics and cumulative IBP effects.
It does NOT preserve full explicit egraph structure, intermediate fact membership,
rewrite trace provenance, arbitrary syntax-sensitive consumers, or extraction
cost. Such consumers need extra state or access to retained histories. It does
not establish correctness of throwing those intermediate facts away. Additional
rules are admissible only if their actions and observations respect this quotient.
