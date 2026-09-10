# Native program block inspector

```
cargo run --bin egg_trace -- egglog/tests/web-demo/cyk.egg results/block_viewer/cyk.json
python3 tools/block_viewer/render.py results/block_viewer/cyk.json --output results/block_viewer/cyk.html
```

Open the resulting HTML or load the JSON through the viewer. Snapshots are taken
at native command boundaries, not individual iterations inside a schedule.
The original schedules, checks, scopes, includes and command order execute in
egglog. Runtime errors are exported and still produce a failing process status.
Scope rollback retires prior block certificates, including rollback inside an
included program. Restored historical rows are boundary inputs, not new producers.

The generic runner registers exact runtime rule descriptions as opaque specs:
it observes dependencies but does not certify equality shortcuts. An invalidated
side output does not invalidate a different committed output of an opaque rule.
Composed rewrites retain the stronger whole-recipe validity gate.

Full CYK (all native checks passed): active blocks before the three scope pops:
24, 36, 66 (including explicit equality interactions). All earlier blocks are retired after each pop. These counts describe
diagnostic blocks, not compression or measured speedup. The viewer lists rejected
match candidates, including action guards that filtered them.

UI regression uses Python Playwright with installed Chrome; run
`python tools/block_viewer/test_ui.py` after generating index.html, cyk.html,
union.html and simplify.html.

## Sample programs and what they exercise

The viewer renders any `egg_trace` export. A sample produces a meaningful picture
when it has either active blocks or an honest rejection list (or both):

| program | blocks | what it shows |
|---|---|---|
| `native_programs/simplify.egg` | 3 active | two `mul-fold` producers joined by `add-fold` (`Const 26`) |
| `native_programs/union.egg` | 2 active | committed/redundant unions, rebuild row versions, equality edge |
| `web-demo/cyk.egg` | 24/36/66 | self-referential `P` span chain; pops retire all blocks |
| `web-demo/herbie-tutorial.egg` | up to 35 | interval bounds + constant folding across scopes |
| `web-demo/fibonacci.egg` | 1 active | self-referential `fib` chain |
| `factoring-multisets.egg` | 0 | container/multiset canonicalization: 5625 rejected, honestly listed |

The scenario selector labels native programs by basename (e.g. `cyk.egg`); the
phase selector shows command number plus the schedule/action (e.g.
`#31 run-schedule …`, `#37 pop 1`). The old hardcoded fixtures keep their Chinese
labels.


Native union/rebuild example:

```
cargo run --bin egg_trace -- experiments/native_programs/union.egg results/block_viewer/union.json
python3 tools/block_viewer/render.py results/block_viewer/union.json --output results/block_viewer/union.html
```

The viewer shows committed versus redundant unions, old/new row versions, and
union event IDs supporting a rebuild. Equality interactions appear as labelled
block edges. Such edges do not automatically certify an equality shortcut.

## Views

The viewer has three views, selected from the `视图` dropdown:

1. **依赖块 (blocks)** — block cards with entry prefix, member dependency DAG,
   combined prefixes, potential exports, boundary reads, and cross-block
   interaction edges. Below the cards: rejected match reasons, committed/redundant
   unions, and rebuild row versions.

2. **覆盖 (coverage)** — the logical coverage ledger. Shows how many live
   constructor rows are covered by prefix views, per-fact 6-state (Covered /
   SeedOnly / Unobserved / Unsupported / Invalidated / Retired), a stacked state
   bar, uncovered rows, per-view marginal new facts, and overlap (duplicated
   memberships counted once, not summed). This is logical coverage, not physical
   storage or a memory-saving claim.

3. **e-graph** — the native egraph serialized to a graphviz dot string via
   egglog's own `serialize().to_dot()`. e-classes are clusters; the dot is shown
   as source and can be saved to a `.dot` file and rendered with `dot -Tsvg`.

`egg_trace` now exports both `coverage` and `egraph_dot` per phase. Coverage is
only produced when the program declares traversable constructor/relation tables;
a container-only program degrades to "no coverage" rather than aborting.
