# Parameterized Analyze queries in the marker solver

Integrates `codex/pass3-parametric-reuse` (`d9da022`) into the existing scheduled
spine solver. This is an actual native/assisted runtime path, not a matrix
prototype or an end-to-end speedup result.

## Contract

A parameterized query has typed slots, complete atoms, correlated aliases,
native guards and the original RHS. For example:

```egg
(rule ((Add-All-Gets orig something offset pos)
       (< pos (tuple-length something)))
      ((union (Get orig (+ offset pos)) (Get something pos))
       (Add-All-Gets orig something offset (+ pos 1)))
      :ruleset always-run)
```

`offset`, `pos` and the current length can be marker fields. The solver can
carry them, or recover omitted values using the existing correlated prefix
receipts. It does not replace them with observed training constants. Literal
thresholds and arithmetic constants stay literals. Distinct variable names do
not impose disequality.

The prefix is shared across consumers, including consumers with different
numeric guards. Each original consumer still checks its own guard at its Run
boundary. Native committed-row subscriptions invalidate records on changed
length/cost, deletion, visibility changes and union/rebuild. No recursive RHS
is moved into an earlier finite Run. A complete-query certificate never grants
whole-saturation skipping.

## Integration

Previously, loading the parameterized library reserved **all** its queries.
The solver therefore could not choose their layouts, and installing a second
selected plan would cause duplicate-provider rejection.

Now:

1. Native-check the imported library. Keep the existing stronger closure
   kernels reserved; expose its general complete-query programs to selection.
2. Map Analyze's anonymous `R0`, `R1`, ... names to source rules by the complete
   LHS, RHS and ruleset. Ambiguous rules or changed actions are rejected. Native
   typed slot wiring and guards are separately checked after this mapping.
3. Use the existing joint layout model: constructor anchor, prefix, optional
   anchor/outside/all payloads, private or shared prefix, shared indexes. Numeric
   slots participate like other typed coordinates. There is no new candidate
   budget, sampling or parameter-value specialization.
4. Replace that query's implementation with the selected plan. Install one
   executor; keep its original actions, frontiers and certificate strength.
5. Write `selected-certificates.egg` (transfer version 4). Its comments contain
   the checked wiring and marker payload; its declarations contain the original
   rules. A fresh runtime can load it without the training input, selection JSON,
   solver, or history. Versions 1–3 remain readable.

`spines.egg` exposes parameter slots and guards; `shared-markers.egg` describes
shared prefix relations. These are descriptions of the runtime layout, not extra
scheduled helper scans. `program.egg` remains byte-identical to the input.
This does not finish the separate explicit `source_sk_mk.egg` source-lowering
experiment or its Pass-3 performance gate.

## Validation

`results.json` records the solver choices for the existing tuple and If fixtures:

- Tuple: five queries admitted; Gurobi proves optimality over 284 layout choices.
- If: one query admitted; SCIP proves optimality over 4,014 choices. The same
  model exceeds the local size-limited Gurobi license (4,869 variables); no
  layouts were discarded to fit the license.
- With default static costs, both small fixtures choose private single-atom
  anchors and no extra numeric payload. **This is not evidence of a numeric
  marker speedup.** The solver-domain regression with explicit high-reuse cost
  inputs chooses shared prefixes carrying a numeric parameter for two differently
  guarded consumers. Those cost inputs are synthetic, not measured timings.
- Native runtime tests additionally exercise full and numeric-only payloads,
  including omitted-field recovery, unseen indices, updated/missing lengths,
  correlated aliases, union, every finite Run frontier, checked overflow and
  stale/tampered plans. Full rows, values, equalities and visibility are compared
  to the same-submit native reference. The selected If program also runs through
  the shared CPU/GPU loop with an actual Metal worker.
- Fresh processes load the solver-selected libraries. Tests reject an altered
  guard, a future/unbound payload, and a source rule with the same LHS but a
  different RHS.

The existing provider still rejects computed LHS primitives and unsupported
container columns. It does not infer arbitrary affine recurrences or replace a
recursive saturation with one operation. Effect evaluation and integer failure
semantics remain native. No new Pass-3 timing or extraction claim is made.

## Reproduce

Use the repository's existing solver Python environment. Build `scheduled_spine`
and `lhs_ast` into `target/analyzer`; the compiler's normal AST entry point is
`target/release/examples/lhs_ast` (a matching build or target-directory symlink).

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo build --offline \
  --target-dir target/analyzer --bin egg_layout
DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo build --offline --release \
  --target-dir target/analyzer --example scheduled_spine --example lhs_ast

target/analyzer/debug/egg_layout analyze-transfer \
  experiments/observed_transfer_queries/tuple-training.egg out/tuple-library.egg
python -m tools.cell_spine.compiler --mode scheduled --static-cost-experiment --skip-build --solver gurobi \
  --source experiments/observed_transfer_queries/tuple-training.egg \
  --certificate-library out/tuple-library.egg --output out/tuple-selected
target/analyzer/debug/egg_layout run-transfer \
  out/tuple-selected/selected-certificates.egg \
  experiments/observed_transfer_queries/tuple-updates.egg out/tuple-loaded
```

The regular compiler uses `--analysis-dir DIR` alternatively to
`--certificate-library FILE`. It returns the selected library path for the
existing bench driver; unchanged closure proofs remain in that library.
This small historical reproduction explicitly opts into the old static model.
Production layout selection now requires `--runtime-workload` from the bench
skill's `planner-corpus` / `planner-prices` pipeline.

## Full Pass-3 follow-up

The [full-input check](pass3/README.md) measures a benefit from the fixed
parameterized programs but a large regression from the automatic static layout.
It documents the wrong-root cost-model defect and the exact incumbent-bound
repair needed to make the full model solvable. Do not extrapolate the tiny
fixture validation above to a Pass-3 marker speedup.
