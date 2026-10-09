# Compile a real Cell's structural boundary response

This experiment compiles **constructor availability and equality**, rather than
an auxiliary size field. It uses recorded Math Cells 8, 10, 15, 36 and 59, keeps
all 24 original rules, and validates emitted structure against actual egglog.
No engine changes, timing benchmark, or production speedup are claimed.

## A union actually connects two Cells

Cell 8 starts with `Mul(a,k)`. Cell 15 starts with `Pow(x,2)` and its original
rule derives and unions `Mul(x,x)`. Cell 36 records the boundary action
`union(k, Pow(x,2))`. That action enables associativity across the old boundary:

```text
Cell 8: Mul(a,k)        Cell 15: Pow(x,2) = Mul(x,x)
           │                          │
           └──────── union ──────────┘
                         ↓
       Mul(a, Mul(x,x)) = Mul(Mul(a,x), x)
                                      ↑
                              new internal constructor
```

Cell 36 also contains the actual Sub context from Cell 10. Cell 59 is its
recorded subsequent commutativity application. Both full entry replays end at
the same **41 Math constructor rows / 14 Math e-classes** in this experiment.
The saved export's 19 values include primitive values; they are not 19 e-classes.

The source facts give four boundary action packets:

| Gate | Actual recorded input action |
| --- | --- |
| A: outer_mul | `Mul(a,k)` |
| S: sub_context | `Sub(b, Mul(a,k))` |
| P: square | `Const(2)` and `Pow(x,2)` |
| U: connect | `union(k, Pow(x,2))` |

Here a, k, b, x are the recorded opaque RipenInput slots. Source hashes, typed
bindings, parent links and member records are retained in the generated report.
The final staged associativity witness is **not injected**: the compiler must
derive it from the earlier inputs, original rules and congruence.

## Exact polynomials describe structural facts

For every ground constructor spelling t, maintain a polynomial `present(t)`.
For every potentially equivalent pair, maintain `equal(t,u)`. Variables are the
four boundary gates, not e-class IDs or expression values. Arithmetic is exact
over integers with `A*A=A`, and similarly for the other gates:

```text
AND(p,q) = p*q
OR(p,q)  = p+q-p*q
```

The original rule ASTs perform conditional constructor insertion and union.
Repeated pattern variables require equality. Congruence and transitivity also
propagate conditional equality. Iterate to an unchanged symbolic state; do not
fit coefficients to native outputs or enumerate truth tables to obtain them.

The resulting examples are:

```text
present(Mul(a,k))       = A + S - A*S
present(Pow(x,2))       = P + U - P*U
equal(k, Pow(x,2))      = U
present(Mul(a,x))       = U*(A + S - A*S)
equal(k, Mul(x,x))      = U
```

The second equation matters: evaluating the union action **constructs Pow** even
when the separate P packet is absent. The fourth equation is an actual
cross-Cell rewrite effect, not an Expr-size calculation or a match count.

## Matrix and incremental execution

Collect the polynomial coefficients into a matrix M over the observed monomial
features `1,A,S,AS,P,U,AU,SU,ASU,PU`. The response is `y=M*features`.
Exact rational row reduction factors M into shared basis responses and their
reconstructions. This instance needs **rank 7, including the constant**.
Coefficients include -1; dropping them produces invalid structural values such
as 2 instead of true/false. No SVD approximation is involved.

There are 2,804 symbolic row/equality coordinates because the compiler retains
alternative ground spellings and their pairwise equality. This is **not** a
2,804-node native graph or a physical memory-compression result. Four gates allow
at most 16 square-free monomials; the small rank is relative to that finite
interface, not evidence that arbitrary egraphs have low rank.

For a new gate g, the exact update is `F[g=1] - F[g=0]`. The executor evaluates
the factored matrix, inserts newly available rows and emits a spanning forest
of additional unions. It does not rerun internal rewrite matching. The compact
full-state emitter uses one action per canonical constructor row and explicit
class slots. See [compiled-all.egg](compiled-all.egg): 41 constructor/union
actions build the complete quotient, with no rule schedule.

The incremental emitter still uses ground-term spellings for new rows; native
hashcons can deduplicate many of those actions. Eliminating that residual action
duplication is separate from the verified elimination of internal rule execution.
Native constructor lookup, insertion and rebuild costs remain.

## What was checked

[results.json](results.json) records the compact results. The native checker
uses non-creating lookups; checks cannot accidentally manufacture expected nodes.

- All **16** gate configurations: complete constructor row set and full equality
  partition agree between original-rule saturation and matrix-selected effects.
- All **32** missing-gate additions: start from an already compiled closure,
  graft the new input, emit only the structural delta, and compare against native
  saturation. The old closures were independently validated by the 16 cases.
- Full recorded entry replay of **Cell 36 and Cell 59** agrees with the all-on
  response. With all inputs except U, there are 23 rows / 12 classes; adding U
  yields 41 rows / 14 classes.
- Missing union effects, omitted constructor rows, false equality and hidden
  rule execution are rejected by negative native checks.
- 18 unit tests cover polynomial algebra, conditional congruence/transitivity,
  repeated-variable guards, exact factorization and unsupported effects.

The runner reports 49 cases: 16 configurations (one also checks full Cell 36),
one extra Cell-59 replay case, and 32 incremental transitions.

## Scope

This is a standalone compiler/executor for **one recorded finite boundary
response**, not general composition of arbitrary Cell operators. The counterfactual
missing-gate worlds vary positive constructor/union actions; they do not preserve
the original replay's staged checks or its failure behavior. The two complete
entry replays provide the actual-history comparisons.

Compilation requires a finite ground-term spelling closure. A finite cyclic
native egraph can represent infinitely many spellings, so it need not qualify.
Compact emission currently requires an acyclic class graph. Function updates,
primitive guards, subsume, arbitrary external grafts, and a varying binding
universe require additional contracts and are rejected/unsupported here.
The monomial space can grow exponentially with boundary gates. No general
complexity reduction, storage compression or GPU execution is asserted.

## Reproduce

The recorded input directory must exist:
`out/observed-transition-math3-verified/events/saturated-rule-composition/cells`.
Use `--cells PATH` to point to the same real records elsewhere. Missing input
fails; there is no fabricated fallback or fresh analyze run.

```sh
cargo build --offline --release --example lhs_ast --example structural_cell_verify
python3 experiments/structural_cell_operator/run.py
python3 -m unittest discover -s experiments/structural_cell_operator -p 'test_*.py'
```

Outputs: `out/structural-cell-operator/{report,manifest,native}.json` and
`compiled-all.egg`. The local modified runtime is used; binary/source hashes
are recorded. The native verifier is `examples/structural_cell_verify.rs`.
`artifacts.py` preserves provenance; `closure.py` interprets source rewrites;
`polynomial.py` handles exact conditions; `response.py` factors the interface;
`run.py` exercises complete and incremental effects.
