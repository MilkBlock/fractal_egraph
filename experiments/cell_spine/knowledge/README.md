# A project library learned from three fact programs

These are actual native Cell compilations, not a simulator. No kernel change or
hardcoded operator family is involved. All programs have the same declarations
and two rewrite rules. Only their facts differ:

1. `independent.egg`: `Join(Pair(a,b), Pair(c,d))`.
2. `small.egg`: only `Pair(x,y)`.
3. `shared.egg`: `Join(Pair(a,b), Pair(b,d))`, sharing one boundary value.

Run once per source, in this order:

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/knowledge/run.py \
  --output out/my-cell-knowledge
```

This needs the existing release `egg_layout`, `lhs_ast`, `spine_validate` and
native storage binaries. It reuses neither an old capture nor a hand-written
Cell library. Results below are functional validation, not a timing comparison;
other regression work was running concurrently.

| Input | New shared states | Previously known states encountered | Imported historical states | Native validation cases |
|---|---:|---:|---:|---:|
| independent | 2 | 0 | 0 | 4 |
| small | 0 | 1 | 1 | 3 |
| shared | 1 | 1 | 1 | 5 |

All 12 native seed/source differential cases passed. The small input really
imports the old larger state into the compiler's optimization and validation;
this is not merely a report counter. Its generated code is still checked against
its own original input. A state can be available in the library without firing
on the current input.

After the third input, the library has three shared saturated states and three
fact programs. Its index is **31,386 bytes** in the recorded run, including
recipes, provenance, graph payloads and learned descriptions. Independent and
shared inputs are represented by one learned hypothesis with three parameters:

```text
states:  [Cell S, Cell S] -> ?h0:CellState
sources: [[Pair($p0,$p1)], [Pair(?h1,?h2)]]
target:  Join(Pair($p0,$p1), Pair(?h1,?h2))
requires: the actual Join connecting those two roots
```

The two recorded bindings differ in whether the second Pair reuses a boundary
value from the first. Shared holes occur in both source and target positions;
the learner does not erase their correspondence. **The target state is still
an observed parameter, not a learned formula for all future target closures.**

The two explicit descriptions cost 346 structural nodes; the selected common
definition plus bindings costs 193. This is a structural description metric,
not bytes, egraph memory or an exponential-compression claim. Transitions are
stored as dictionary calls and residual bindings; decoding verifies the exact
original description hash. Graphs/recipes remain separate validation evidence.

The library supports exact historical Cell reuse now. Generalized hypotheses
do not yet generate arbitrary-depth FractalComb rules or skip new tier0 capture.
No new runtime speedup is claimed. A more complex exploratory `Link` input with
a shared boundary reached 263,995 layout evaluations in its first conditional
problem and was manually interrupted before validation/publication. No search
budget was added; larger cross-input layout domains remain a real cost.

To inspect without running analysis:

```sh
out/cell-spine-venv/bin/python -m tools.cell_spine.knowledge list \
  --source experiments/cell_spine/knowledge/shared.egg \
  --library out/my-cell-knowledge/knowledge.json
```

In VS Code, use **Egg Fractal Analyzer: List Merge Families for Active .egg**.
The command shows only families with evidence for the current source hash;
source edits require a new Bake. Use **Reload Window** after updating the local
extension. Snapshot archive export never contributes learned evidence.
