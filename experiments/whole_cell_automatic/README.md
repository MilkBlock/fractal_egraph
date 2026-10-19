# Native-event-driven whole RipSatCell execution

This extends `whole_cell_chain`: the compiled **complete closure** emitter is
unchanged, but runtime code no longer chooses `advance("B", bindings)`.
Native committed constructor rows and union/rebuild changes drive a multi-instance
registry and automatically select the baked A → B → C transitions.

The example has two held-out input instances. Three ordinary native coarse rules
create their initial Add, then Mul, then outer Add. Those rules do not name any
Cell states or transition IDs. After each coarse step, the dispatcher finishes
known complete Cell transitions; an independent original engine saturates `math`.

```sh
cargo run --offline --release --bin egg_layout --target-dir target/analyzer -- whole-cell-auto-check \
  experiments/whole_cell_automatic/math.egg \
  experiments/whole_cell_chain/plan.json \
  experiments/whole_cell_automatic/heldout.egg \
  experiments/whole_cell_automatic/grafts.egg \
  out/whole-cell-auto

cargo test --offline --test whole_cell --test whole_cell_automatic --target-dir target/analyzer
```

Use a new output directory. This command uses native egglog, no GPU worker. The
check runner adds a **complete `math` saturation boundary after each coarse run**;
it does not pretend that a macro is interchangeable with an arbitrary finite
`(run math N)`. `original.egg` records that precise reference schedule.

## Execution

1. Bake still derives full bodies and verifies each constructor/union delta
   against independent native saturation. The plan only describes graft shapes.
2. Subscribe to existing native commit journals; bootstrap existing rows once.
   No logical tracing or repeated whole-egraph export is used for dispatch.
3. Flatten each known graft into typed constructor atoms. Changed rows anchor
   incremental residual queries; indexes on bound columns complete the graft.
   Source state/slot indexes find only compatible live Cell instances.
4. A newly published source also wakes matching already-present graft facts.
   This handles a whole nested expression arriving before its intermediate Cell
   instances have been registered. Multiple successors can use the same source.
5. Exact input/alias checks precede the existing native effect emitter. Its typed
   receipt supplies the **entire** target frame; state publication follows native
   hashcons/rebuild and equality validation. Small source views remain available;
   dominance does not delete a source that another successor may still need.
6. Typed union events re-canonicalize affected instance frames. Collapsed,
   unbaked alias partitions invalidate certificates. Bound transition instances
   are deduplicated; inserting the same facts does not re-run their macros.

`install_grafts` / `step_grafts` provide the native coarse-rule driver.
`commit` also accepts ordinary constructor/union actions. Neither API asks the
caller to name a target Cell. The matcher for coarse rules still executes and
is counted separately. The original smooth rules remain installed.

## Why fallback is necessary

A valid instance certifies a positive subgraph; it does **not** seal off foreign
context. Two individually saturated Cells can activate a new cross-Cell match.

For nested rewrites and single connected constructor/equality premises, the
runtime checks affected components of active facts (constructors mentioned in
smooth LHS). Inert input rows are excluded. A component may skip original
saturation only when **one** valid whole-Cell instance contains *all* its active
rows. Components connect through typed arguments and result values, including
literal positions. Owner intersections verify this condition without a subgraph
isomorphism search. The proof relies on the restricted positive rules: every
complete LHS lies in one such component, and its certified closure already
contains its RHS and equalities.

If components span multiple partial certificates, an unknown alias/graft appears,
or the rules contain multi-fact joins, the runtime conservatively saturates the
original ruleset. This fallback is explicit in the report. Multi-fact joins are
not claimed incomplete or impossible; this implementation simply does not yet
prove their coverage. Fallback-produced events remain available to dispatch at
the next input boundary. They can add certificates but are not credited as
skipped native work.

## Checks and limits

- Compare **every original constructor row, literal and equivalence class** at
  every boundary, not just counts or one extracted term.
- Report original query watermarks, actual native coarse/smooth match counts,
  macro worklist matches, fallback counts, and source/target instance IDs.
- Tests cover interleaving, branches, non-root grafts, nested input arriving at
  once, no-op reinsertion, aliases, known quotient transitions, unrelated
  instances surviving union, typed frames/literals, and cross-Cell/disconnected
  joins requiring fallback.
- CPU/native semantic integration, **not a GPU acceleration claim**. Residual
  graft queries are currently an indexed CPU join. The mirror, column/ownership
  indexes and affected-component walks are real time/memory costs.
- All original facts remain materialized. There is no virtual-storage claim.
- The same positive constructor/union and complete-saturation restrictions as
  `whole_cell_chain` remain. Bake does not certify primitive guards, custom
  function updates, destructive operations, or infinite unfinished closures.
- The library still comes from supplied coarse graft shapes. It does not
  automatically invent every new Cell or turn all analyzer output into proofs.

Verified result: [RESULTS.md](results/RESULTS.md). The original 10 whole-Cell tests
and 14 automatic-dispatch regressions pass. The sample has 52 native smooth
matches versus zero on the automatic path, with 12 macro worklist calls and
exact whole-graph equality at every boundary. Coarse queries and residual graft
queries still execute; these figures are not an end-to-end speedup estimate.
