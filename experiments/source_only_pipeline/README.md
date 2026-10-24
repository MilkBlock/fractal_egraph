# One-way source pipeline

The executable entrypoint now consumes a self-contained program:

```
source.egg → source_sk.egg → source_sk_mk.egg → cell-assisted-egglog
```

Each compiler stage has one program input. The marker stage profiles **that SK
program**, verifies alternative plans at its original Run boundaries, and feeds
those measured counts to the existing solver. It does not read an older Analyze
catalog, fixed eight-query list, cross-workload profile or preselected layout.
Compiler-generated files remain useful diagnostics, but execution needs none of
them. Gurobi and SCIP use the existing common model.

## Removed from the executable path

- `program_assistance.rs`: implicit selection-closure discovery plus sidecar plans.
- `--setup-policy legacy-stable`, `--boundary-programs` in `--mode program`.
- Environment-loaded boundary/certificate files in program mode (rejected).
- `to-hoist`, `Smaller`, and IVT name-based rewrites in the structural-key stage.
- Automatic `terms` ruleset-name detection in the scheduled/source marker compiler.
- The `explicit-keys-bench` command that reinstalled these services implicitly.
- The external-Analyze, two-atom adapter in `structural-pipeline`.

Generic indexes, event subscriptions, path retention, the parameterized query
executor and explicit source publishers remain. Source-derived decisions select
their inputs. Historical experimental APIs and reports remain separate; none is
implicitly invoked by the new source pipeline. In particular there is no
built-in list of 84 loop instances.

## Source contract

`cell-structural-key-runtime/v1` declares portable key primitives and helper
rulesets. Maintenance rules and schedule are executable source. Key generation
uses constructor schemas and preserves the original merge/choice policy. Sorts
transitively requiring unsupported field encodings are listed and left alone.
This no longer forces identical choices for an originally order-sensitive merge.

`cell-runtime/v1` contains the exact parsed command sequence, complete typed
query plans, selected anchors, slot mappings, guards, cache choices, and marker
table names. The copy of commands is compared, never executed. All plans are
validated against the actual native query compiler. It is not a cryptographic
signature: a new program can have a new validated plan. A statement in a comment
cannot authorize skipping unmatched LHS conditions or skipping saturation.

Only certified positive source prefixes become physical markers. Their producer
rules publish them directly; source completion rules handle initial facts and
new union aliases. Mutable ports stay residual. The same selected-source backend
now seals **all** its output files, including private-only plans.

The runtime reads these declarations automatically on `--backend cpu` or `gpu`.
`native` and `pristine` are unassisted references. `--cell-assistance off` is a
same-source ablation. The execution region starts at the first Run after the
last rule/schema/global-binding declaration; earlier initialization stays native.
All subsequent Runs, actions, checks and extraction stay in original order.
A new AST input method avoids printing/reparsing internal names.

`cells.json` records declared query contexts, actual source marker row counts,
live instances, port reads and cache reuse through the existing runtime report.
These are **complete-query contexts**, not newly discovered RipSat closure
certificates. The old named shortcuts receive no credit in this path. Offline
source profiling and solver work are compilation costs, not runtime speedups.

## Reproduce

From this worktree, using the existing bench skill:

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py \
  structural-pipeline \
  --source experiments/parameterized_source_markers/counter.egg \
  --output out/source-only-example --solver scip

/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py \
  program-modes --backend cpu \
  --source out/source-only-example/source_sk_mk.egg \
  --output out/source-only-example/run
```

For GPU use `--backend gpu --worker <compiled matrix worker>`. The direct runtime
needs only `--mode program --backend cpu|gpu --source FILE --output DIR`, plus
`--gpu-worker` for GPU. No analysis or layout path is accepted.

## Validation

Actual native CPU/GPU runtime tests, not a standalone simulation:

- Automatic profiling/selection from the numeric State/Port fixture: two selected
  complete queries; zero forced source markers. This is **not** evidence of a
  shared-marker performance gain.
- Remove original/SK source and every compiler sidecar, then execute only the final
  file in fresh processes. Compare all rows, function values, equality and
  visibility against native, plus the original program's public tables.
- Six finite Runs with checks between them, changing a function value, adding a
  disconnected input and later unioning it. No premature recursive expansion.
- Source publisher fixture: real marker rows and successor publication, independently
  sealed and executed without its compilation directory.
- Changed executable source, invalid plan anchor, duplicate declarations and
  external overrides reject execution.
- Renaming the constructors and consumers, including using `terms` as a ruleset
  name, does not install a selection closure or change the discovered query count.

The compact checked result is in `results.json`. This change does not claim a new
Pass-3 speedup, universal source lowering, or automatic inference of new saturated
Cell theorems. The solver can still select private contexts rather than shared
markers. Unsupported source transformations fail explicitly; unsupported query
shapes that need no rewrite retain original matching.
