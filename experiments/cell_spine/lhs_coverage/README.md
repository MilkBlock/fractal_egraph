# Broader Bake entries from rule LHS coverage

This experiment adds a typed coverage index to the production Python Bake
compiler and emits additional runtime `.egg` entry rules. It runs the **actual
native egglog runtime** for capture, saturation and whole-state comparisons.
There is no change to the engine, no new closure family, and no original-rule
fallback during compact execution. This is an applicability experiment, not a
speed or memory benchmark.

## Reproduce

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/lhs_coverage/probe.py \
  --output out/my-lhs-entry-transfer

out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_lhs_coverage
```

Native binaries and the compiler's existing solver dependencies must be built,
as for the other Cell-spine probes. Each probe uses a fresh output directory.
It Bakes the original training input once per fixture. Both modes use that same
analysis catalog. **No test input is analyzed, ripened, added to the library or
shown to the solver.** Only the fixed generated library executes on test facts.

To enable the feature for another supported single-ruleset Bake:

```sh
out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/math_tiny/source.egg \
  --output out/math-with-lhs-entries --lhs-coverage entries
```

`--lhs-coverage off` preserves the previous default. `report` generates only the
coverage table; `entries` also emits and validates alternative entrances.
`lhs-coverage.json` contains the operator-to-LHS-position index, source rules,
typed bindings, covered row/condition positions, residual conditions and proofs.
Executable entry rules are in `library.egg`, named `lhs_entry_*`.

## What is learned

Train with P(x), under P(x) -> Q(x) and Q(x) -> P(x). The original capture records
only the insertion of Q: the reverse action finds P already present and supplies
no new committed entry history. The closed Cell contains both P and Q, but the
old compact library only recognizes a raw P input.

Coverage finds Q(x) in the completed closure. Its rule RHS supplies the known P
seed, so the compiler can generate the following kind of entry:

```egg
(rule ((= incoming (Q! p0)))
      ((P!! p0) (Summary-0! (P!! p0)))
      :ruleset Run! :name "lhs_entry_0_0")
```

The actual selected marker layout determines the emitted fields. This is a new
entry into the existing Cell, not a new Cell or a fabricated trace event.

A guarded LHS Q(x), Gate(x) is reported as partial when Gate is absent. Its
residual condition is retained; it does not authorize an Out result. Likewise,
Q(x), R(x) cannot be assembled from Q(a), R(b) while a and b are unequal.

## Why LHS coverage alone is insufficient

If a new LHS merely contains an old seed, the old entry often already applies.
The useful test also looks at the RHS of that same rule:

- K is a certified seed with Sat(K) = D.
- Match the rule's complete LHS inside the finite closed D, preserving bindings.
- Form E from this grounded LHS plus K facts not supplied by its grounded RHS.
- Keep all seed equality guards; validate all new facts and equalities inside D.
- Require every old input port to remain bound in E.

Then E is contained in D, and one legal rule application supplies K. In the
accepted positive constructor/equality environment, monotonicity gives
Sat(E) = D. Every emitted entry also passes the existing native full-state
validator before the compiler publishes executable output.

Native rewrite rules name their LHS root. That variable must use the matched LHS
expression, not an arbitrary equivalent representative from D: otherwise a
Q->P entrance could accidentally require P already. The implementation resolves
acyclic LHS definitions and keeps any remaining equalities explicit.

Identical entries are deduplicated in family/port coordinates. An entry already
covered by a known family seed is omitted. The pass does not recursively feed
new entries back into discovery or enumerate arbitrary subsets of LHS atoms.

## Results

| Fixed training library | Before | With entries | Cell families before / after | Library bytes before / after |
|---|---:|---:|---:|---:|
| P / Q, unseen input names and missing Gate | 2/4 | 4/4 | 1 / 1 | 3,601 / 3,748 |
| Reversed argument positions | 1/3 | 3/3 | 1 / 1 | 2,842 / 3,004 |
| Shared argument and union enablement | 1/3 | 3/3 | 1 / 1 | 3,051 / 3,225 |
| Multi-fact Q(x), R(y) entrance | 2/4 | 4/4 | 1 / 1 | 3,230 / 3,416 |
| Math microbenchmark tiny subset | 3/4 | 4/4 | 3 / 3 | 10,603 / 11,695 |
| Total | 9/18 | 18/18 | unchanged | |

The cases include negative controls, not only newly admitted inputs. Nine
previously failing cases became complete; the other nine remained correct.
All comparisons cover the complete constructor graph and equality partition,
not just node counts. Compact inference executes no original inference rules;
explicit Export restores the logical graph for comparison.

The Math training input is x*(y+z), with the existing four-rule commutativity /
distribution / factorization fixture. A held-out input a*b+a*c previously restored
9 constructor rows against the native 13; the new entry restores all 13 and the
same equality partition. Different coefficients a*b+d*c still do not falsely
factor. The original factored and commuted inputs already worked and still do.

## Bounds and limits

The index uses the native AST, constructor signatures and actual eclass IDs. A
shared variable must keep the same ID across all atoms. Full LHS matches are
finite e-matches in the completed snapshot. Partial rows are **anchored partial
witnesses**, not an enumeration of all maximal partial combinations. Covered-row
counts do not by themselves certify equality guards or a complete rule match.

This remains an exact join problem: a rule with many independent variables can
have combinatorially many bindings. There is no Top-K, arbitrary search budget,
or claim of linear-time discovery. The option stays explicit for this reason.
The table reports snapshot row visits separately from native runtime metrics.

New entries are admitted only in the positive constructor/equality fragment.
Primitive guards, destructive effects, observation-only states, fixed named ports, and automatic
transport of several already-aliased opaque input ports are outside this proof.
We use finite existing terms to express bindings, not infinitely many equivalent
spellings. Only one-rule replacements of existing seeds are considered here.

The result broadens routes into **known** closed Cells. It does not establish
coverage of arbitrary new contexts, new closure sizes, or all inputs of a rule
set. Inferred entry edges are kept separate from observed history in the
persistent merge-family learner; they are recomputed from stored closures and
the current rule environment rather than recorded as invented match events.
