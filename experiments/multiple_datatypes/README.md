# Multiple equality datatypes and real eggcc history

The native analyzer accepts multiple `datatype` declarations, mutually recursive
`datatype*`, and explicit `sort` + `constructor` declarations. Each captured
value/union retains its equality sort. Ripen allocates a separate, typed
`RipenInput_N` constructor per equality sort and exports cross-sort rows.
Presort/container declarations remain intact for native execution; exact ripen
state export still rejects unsupported container fields rather than mis-typing them.
The existing single-datatype state scope is preserved for old libraries.

## Reproduce offline

From the repository root, with eggcc already built locally:

```sh
cargo build --offline --bin egg_layout
python3 experiments/multiple_datatypes/run.py \
  --eggcc /Users/mineralsteins/Repos/egg_related/check_statewalk \
  --output /tmp/eggcc-multisort-run
```

Use a new output directory per run (saved histories are deliberately immutable).
The script exports the **real** `fact.bril` optimizer program for one pass, executes
its original schedule, saves resolved history, and re-analyzes that history.
No graphics, external downloads, synthetic rewrite replacement, or round override.
Generated source and full history stay in the output directory, not Git.
`results.json` records source hash, eggcc revision, commands, and measured results.

## Verified result

- 5,695 source lines, 95 equality sorts (96 sort declarations including a presort).
  The exporter lowers relations into additional sorts; this is not the original
  schema's datatype count.
- 4,664 logical matches, 2,777 imported applications; 1,887 excluded by existing
  trace eligibility checks. Import requires surviving action lanes, complete
  physical witnesses, supported writes/unions, and resolvable binding sorts.
  Logical matches and imported applications are **not** counts of changed rows.
- 2,060 composition jobs; live analysis and saved-history replay agree.
- **0 certified saturated cells**: all 2,060 jobs encounter the pre-existing
  global merge-update audit (`pre-update table-state witness` required).
  Removing the datatype gate does not remove that independent correctness limit.
  This experiment proves real trace ingestion/replay, **not cell reuse on eggcc**.
- Debug-build timing is diagnostic only, not a runtime speed benchmark.

Real-history replay also exposed missing `subsume` action source paths. These are
now encoded as stable `head/N/change/0` paths, so action receipts can be restored
without guessing AST positions.

Focused tests cover all three declaration forms, correctly typed unions in two
sorts, history replay including subsume, and native saturated state export with
cross-sort constructors, a relation, and an f64-valued function. Existing Math,
reentry, composition, catalog, and runtime tests remain part of regression checks.
