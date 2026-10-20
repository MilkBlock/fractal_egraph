# Repair: Cell supplies matches, not another execution order

The old selected-Run path called `step_cell_mixed_candidates`: selected scalar
rules used Cell ports, while remaining rules used native fused queries. That
silently changed the sorted-commit protocol of the all-rule GPU runtime.

A small regression proves this independently of saturation and no-op pruning:
combine a scalar `Size` rule with `Pick(x) :merge new`, where several `Bop`
rows supply different `y` values. Construct leaves and parents in opposite
orders. The old Cell path chose a different `Pick(x)` from the sorted CPU
reference, even though `Pick` was not the accelerated rule. The test is
`cell_run_preserves_the_sorted_commit_of_other_order_sensitive_rules`.

## Corrected contract

1. The compiled Cell port program supplies complete incremental LHS bindings
   for the supported scalar rules. It evaluates exact LHS guards, including
   checked-overflow rejection.
2. The GPU (or CPU reference) finds bindings for all remaining rules against
   the same frozen state.
3. Merge these binding lists, sort them by the original rule/binding protocol,
   and submit every original RHS in one native commit.
4. Advance original and independent-audit frontiers together. Switching Cell
   reuse off and back on cannot replay old effects.

The provider does not prune min no-op RHSs, change how remaining rules execute,
or run future scalar waves inside an ordinary Run. It preserves RHS errors.
The separate certified joint-saturation handler remains at Saturate boundaries.

This path is enabled by default for `--all-rules --joint-sat`.
`CELL_DISABLE_CELL_QUERIES=1` disables the query reuse for ablation. The old
`CELL_EXPERIMENTAL_RUN_PROGRAMS` mechanism was removed; old results remain
historical evidence, not the current implementation.

## Real Pass-3 result

One fresh candidate run reused the exact saved controls under
`out/pass3-compact-layout`:

| Metric | Before | Fixed Cell frontiers |
|---|---:|---:|
| Inference seconds | 22.585 | 22.636 |
| GPU device seconds | 8.652 | 7.287 |
| CPU/GPU batches | 4,059 | 2,761 |
| Downloaded binding bytes | 65,637,404 | 23,834,036 |
| GPU query requests for the 17 covered rules | 21,732 | 0 |
| Complete bindings for those rules | 1,918,606 | 1,918,606 |

Every rule's submitted-binding count is unchanged, as are the complete final
facts, values, equivalence partition and visibility relative to the same-commit
CPU and GPU-off controls. Production Tiger's selected CPU/GPU DAGs also match.
A separate **Cell-only native frontier audit checked 30,566 queries**, including
empty frontiers, with no missing or extra bindings.

There is no meaningful end-to-end speedup yet. Reduced GPU search and transfer
are accompanied by CPU port/maintenance work. Rule-query requests are not GPU
kernel launches, and complete bindings are not committed insertions. This repair
establishes the correct interface for further Cell optimization.

The existing difference from fused-native execution and its Tiger result is
unchanged. Both extraction gates are now always written: `quality.json` keeps
the fused-native result; `cpu-gpu-quality.json` records the same-commit pair.
The old failure exit code is preserved, not changed into a pass.

[Saved metrics and fingerprints](cell-query-fix.json). Regression tests also
cover finite Runs, stateful observers, fallback/resume, union, LHS overflow and
RHS overflow even when the proposed min write would be a no-op.

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py audit \
  --all-rules --joint-sat --cell-only --output out/new-cell-frontier-audit
```
