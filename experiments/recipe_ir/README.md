# Shared Recipe IR and eggfuse components

This is a correctness/integration change, not a new Pass-3 speedup result.
The actual compiler and Metal runtime sources have moved to their component
locations. The initial migration retained compatibility modules/symlinks; the
subsequent cleanup removes them after migrating callers.

## Implemented

- Canonical project entry: `python3 -m eggfuse` (`analyze`, `compile`, `run`,
  `run-cells`, `report`, `inspect-recipe`). Rust `egg_layout` remains compatible.
- `eggfuse-recipe/v1`: correlated typed slots, complete queries, ordered effects,
  original control, direct/merge footprints, and table-shared event routes.
- `eggfuse-recipe-library/v1`: adapter for actual native initialized Cell exports,
  preserving source versus isolated-ripen scopes and completion status.
- Analyzer entry and compiler Analyze pass publish the common library. Both
  ordinary/local lowering and existing terms admission consume the common
  reference program. Device loading checks that same contract against source.
- A self-contained device artifact includes source, marker, Recipe IR, typed
  core, capabilities, optional specialized recipe and shader. A fresh runtime
  loads it without invoking the device compiler again.
- `capabilities.json` separates representation/verification/lowering and available
  fast scopes. Actual execution is unknown at compile time, not invented as a hit.

## Verified

1. The entire saved Pass-3 typed program roundtrips exactly: **548 exported rule
   entries, 30 rulesets** (including initialization/empty debug), **157 shared
   event routes**. These are program entries, not distinct Cell families or
   proof of optimized matching coverage.
2. Full Pass-3 kernel text is byte-identical after Recipe IR roundtrip and source
   migration: `f90ce0cd41b22543ab436a36eeee2f23b172bae610819b43043cbb30ffe06873`.
   This is a no-algorithm-change check, not a fresh performance measurement.
3. Five fixtures use the **unchanged 17 real eggcc context rules**: saturation,
   finite one Run, late union, context-to-terms, and symbol renaming. All compare
   complete facts, function values, equality and visibility with independent
   native source execution. Not all 17 rules necessarily fire on each input.
   Context-to-terms executes 49 Runs and ends with 229 rows / 181 classes.
4. Real native Analyze of the context-to-terms input exports 144 initialized
   Cells. All **132 Saturated** instances match separate native oracle states
   after execution from the new library in a fresh GPU process. Twelve
   `ObservedUnfinished` instances remain excluded and listed. One shared typed
   program serves those instances. The original source program also matches.
5. Existing generic execution tests pass: 11 semantic cases, two batched instances,
   and eight dynamic capacity/failure/control checks. They include correlated
   relation keys, i64 updates, custom merge, growing structure, late union,
   visibility and failed checks that cannot be rescued by a later stage.
6. Fifteen IR/cache/contract unit tests pass, including re-signed corrupt wiring,
   forged guarantees/footprints, scope promotion, ambiguous typed calls, and a
   valid different program substituted for the expected source.
7. Source-marker compatibility and native/GPU tests pass, including the then-supported old imports,
   immutable publisher/completion, late union, alias fibers, finite Run, local
   execution and cache reuse. A copied artifact runs from a separate temporary
   directory while the lowering function is patched to fail if called.
8. Existing multi-stage terms remains active through the canonical compiler:
   214 rows / 182 classes, 31 patches, 12 boundary actions and 26 update actions;
   full native equality holds on this small real-rule input.

## Explicit boundaries

The new event routes are **not installed as generic runtime subscriptions**.
They require actual post-merge visible rows, same-row column constraints, residual
queries and original stage gates. They do not claim a marker already exists or
that its remaining conditions are true. Generic execution still performs queries.

This change does not certify live global substitution for isolated endpoints,
does not make all context/always-run rules fast paths, and does not repair the
existing full Pass-3 native/device mismatch. The terms-specific optimized
certificate remains specialized. Rust crate extraction and separation of the
legacy host/oracle harness are later migration steps.

## Reproduce

```sh
python3 -m unittest eggfuse.ir.test_recipe eggfuse.compiler.test_passes \
  eggfuse.compiler.test_multistage -v
python3 experiments/recipe_ir/check_context.py \
  --source /path/to/original/raytrace-pass-3-flat.egg \
  --build out/device-build-scheduled-reviewed --output out/new-context-ir
python3 experiments/device_backend/check_generic_analyzer.py \
  --source out/new-context-ir/context-to-terms.egg \
  --build out/device-build-scheduled-reviewed --output out/new-analyzed-context-ir
python3 experiments/unified_compiler/check.py \
  --build out/device-build-scheduled-reviewed \
  --solver-python /path/to/scip-python --output out/new-component-checks
python3 -m eggfuse compile --source out/cell-update-pass3/training.egg \
  --output out/new-terms-ir --build out/device-build-scheduled-reviewed \
  --solver-python /path/to/scip-python --capacity 4096 --match-capacity 16384 \
  --multi-stage-cell
```

Raw reports are under `out/recipe-ir-real-context`, `out/recipe-ir-native-context`,
`out/recipe-ir-generic-tests`, `out/recipe-ir-boundaries`,
`out/eggfuse-components-final`, and `out/eggfuse-final-terms`. No generated binaries,
mutation streams or large input/endpoint graphs are tracked.

## Legacy path removal (2026-10-29)

Removed 55 forwarding/source-alias paths: the old `tools.eggfuse` package,
18 Python compatibility modules and 25 Swift/Metal symlinks. Current scripts,
source links and the profiling skill use the canonical components. Historical
result JSON remains unchanged. The real frontend/host/oracle harness under
`experiments/device_backend` and Rust analyzer still exist; they are not aliases.

Validation after deletion:

- 17 shared-IR/pass/layout tests pass, including six direct scripts launched
  outside the repository without `PYTHONPATH`.
- A fresh worker build reads `engine/metal`; its generated Swift is byte-identical
  to the previous component build. The manifest records actual source files and
  preserves the reused native frontend's original fingerprints.
- `experiments/unified_compiler/check.py` passes with the rebuilt worker:
  native/GPU checks, local backend, marker publication, artifact loading without
  relowering, cache reuse and rejection of altered contracts.
- Polyspine marker unit checks: five pass; three native/Metal-worker cases remain
  skipped without their optional fixtures. Profiling skill validation and its
  eight unit checks pass.

Reproduce the path-sensitive unit checks with:

```sh
python3 -m unittest eggfuse.ir.test_recipe eggfuse.compiler.test_passes \
  eggfuse.compiler.test_multistage eggfuse.compiler.test_layout -v
```

This cleanup changes source ownership and entry points, not inference algorithms;
no new Pass-3 timing or whole-program equivalence claim is made.
