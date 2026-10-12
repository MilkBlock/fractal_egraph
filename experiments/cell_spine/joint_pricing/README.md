# Joint spine, marker and subtree-reuse pricing

The old choices are frozen from `2402786`; both old and new choices are emitted
by the current compiler and evaluated with **the same formula**. Source, path,
layout and implementation hashes reject a stale reference. No approximate
selection mode has been added to the production compiler.

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/joint_pricing/run.py \
  --output out/joint-pricing-next --skip-build
```

Omit `--skip-build` if the native tools are not built. This runs native analysis,
Gurobi, code generation and full native egraph comparison. Each case directory
contains the new `library.egg`, `runtime.egg`, `report.json` and `validation.json`.
Its `reference/` directory contains the regenerated old-choice library and
independent native validation. This is not a standalone graph simulator.

## Same-formula results

Unit weights: shared template shapes + stored marker columns + runtime read
terms/guards + runtime write terms/unions/changes/marker columns. Costs include
all inherited consumers and retirement rules after actual subtree substitution
and safe decoder elimination. Explicit export and input construction are not
part of this objective.

| Case / choice | Template shapes | Marker columns | Reads | Updates | Total |
|---|---:|---:|---:|---:|---:|
| 3-input AC, old | 2 | 2 | 25 | 50 | 79 |
| 3-input AC, joint | 2 | 2 | 25 | 50 | 79 |
| Long-spine graft, old | 7 | 2 | 26 | 26 | 61 |
| Long-spine graft, joint | 7 | 3 | 19 | 28 | 57 |
| eggcc If subset, old | 17 | 11 | 1859 | 1576 | 3463 |
| eggcc If subset, joint | 17 | 21 | 711 | 1978 | 2727 |

The long-spine fixture has seven inputs and an external `Extend` graft. It is
**not** the full seven-input AC program. The new target marker stores its root
and the reusable old subtree instead of only the root:

```text
old: SummaryBig(extended_root)
new: SummaryBig(extended_root, old_chain_root)
```

Later consumers can obtain the old chain directly. The solver accepts one more
stored field and two more update units because it removes seven read units.
The If case similarly spends more on payload and writes to remove reads. These
are code-structure scores, not runtime match counts, speedups or RSS savings.

## Validation and model size

| Case | Native cases per variant | Original/restored e-nodes | Original/restored e-classes | Outer vars / constraints | Peak inner vars / constraints | Priced schemes |
|---|---:|---:|---:|---:|---:|---:|
| AC | 10 | 15 / 15 | 7 / 7 | 74 / 122 | 31 / 53 | 14 |
| Long graft | 3 | 18 / 18 | 18 / 18 | 12 / 28 | 102 / 343 | 1 |
| If | 9 | 60 / 60 | 52 / 52 | 74 / 130 | 180 / 456 | 6 |

Every native full-state comparison passed for **both** variants. The compressed
execution did not run the original inference rules. Native-parsed emitted rule
sets and their costs also exactly matched the solver's priced ASTs.

These models fit the locally verified restricted license limit of 2,000
variables and 2,000 linear constraints **per model**. Conditional subproblems are
separate models, so their sizes are not added together. Outer path decisions
receive exact inner costs and may change until the lower bound meets the
incumbent, subject to Gurobi's numerical tolerances. This is not a greedy
spine-first selection.

The cost diagram compresses repeated cost suffixes; it does not remove all
combinatorial work. Before diagram sharing, a rule may require the Cartesian
product of its participating families' marker layouts. The outer solve may
visit every legal path/implementation choice. No sampled candidate cap or silent
license workaround is used. The source whitelist and layout-form limits remain
those in `tools/cell_spine/README.md`.

Independent finite tests compare conditional optimization with exhaustive
enumeration, test joint endpoint marker choices, and test every assignment in a
cost diagram. Native regressions check subtree anchoring, late external inputs,
union, shared tails, marker retirement and inherited consumers.
