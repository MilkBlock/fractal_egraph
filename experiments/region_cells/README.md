# Generic multi-stage Spine regions

The analyzer emits a typed `region-program.json` rather than a specialized
`terms` closure library. The canonical compiler's explicit `--multi-stage-cell`
option consumes this plan and selects the `regions` runtime. Ordinary compilation
without that option retains the existing GPU backend. Legacy terms experiments
remain available through their standalone experiment runner for historical comparisons;
the canonical engine rejects their specialized switches/artifacts. The compiler no longer imports
`recipes.executable` to implement this option.

## What executes

- The analyzer constructs rule dependency SCCs, preserves the original control
  tree, and extracts alpha-normalized keyed Spine prefixes inside individual
  rules. Numeric guards and branching joins remain exact residual queries;
  a mixed ruleset is never rejected merely for failing a predicate-only pattern.
- Prefixes form a shared DAG. A child extends its parent's complete correlated
  binding. A keyed extension has at most one row per anchor, so storage does not
  grow by reserving every row × every plan or by taking a Cartesian product.
- GPU insertion, function updates, subsumption and congruence rebuild publish
  table versions. Only plans reading changed tables become stale. Consumers
  lazily extend these plans at their original Run boundary. This is not yet
  per-key subscription dispatch or eager marker publication from each effect.
- Actual fact IDs and timestamps are retained. After a table changes, unchanged
  witness tuples can be copied using a shared fact-ID-to-row map, avoiding key
  lookup. Removed/aliased/subsumed witnesses must be recomputed. Missing keyed
  inputs are retried when their table changes; absence is never permanent proof.
- The packed marker arena grows dynamically and reclaims superseded blocks.
  Cache-allocation failure resumes the same untouched Run before RHS. Completed
  stages are not replayed. Existing row/match-capacity failures still use the
  generic runner's private-instance retry; no result is published twice.
- All markers in one original program refer to the same GPU identity domain.
  Constructor requests receive actual hashcons identities in recipe registers;
  same-key updates execute the original ordered merge, including intermediate
  constructors. Deferred unions and rebuild run on GPU at logical Run boundaries.
  Cross-stage results stay in this arena, and no CPU stage search occurs.

A region is an analysis grouping, not permission to reorder its rules. SCC edges
include conservative union effects and may over-connect regions. An inner Run
still collects all matches before executing any RHS. Finite Repeat and nested
Saturate keep their original stopping rules. Final table export occurs at the
program boundary; this does **not** delay every local merge/rebuild until then.

## Important execution boundary

This is a general semantic/optimization substrate, **not the finished throughput
backend**. Within a shared arena the control executor is still serial; separate
independent arenas may run in parallel. Residual joins, table-granularity refresh,
ordered RHS and logical-Run rebuild remain. Do not run full Raytrace with this
option expecting the old specialized timing. No full Pass-3 speedup or repair of
the pre-existing native/device whole-graph difference is claimed.

## Validation and reproduction

```sh
python3 -m unittest eggfuse.ir.test_recipe eggfuse.analyzer.test_regions \
  eggfuse.compiler.test_passes eggfuse.compiler.test_multistage \
  eggfuse.compiler.test_layout -v
python3 experiments/region_cells/check.py \
  --build out/eggfuse-clean-paths-build --output out/new-region-checks \
  --real-dir out/recipe-ir-real-context
python3 -m eggfuse compile \
  --source experiments/region_cells/smoke.egg --output out/new-region-pipeline \
  --build out/eggfuse-clean-paths-build --solver-python /path/to/scip-python \
  --multi-stage-cell --capacity 256 --match-capacity 1024
python3 -m eggfuse run --artifact out/new-region-pipeline/eggfuse_related_files \
  --build out/eggfuse-clean-paths-build --output out/new-region-reload --check
```

`--real-dir` is optional. To regenerate its four real-rule fixtures, use the
existing `experiments/recipe_ir/check_context.py` with a saved Pass-3 source; it
retains the actual rule bodies and changes only inputs/schedules. No network or
full Raytrace inference is needed. Worker compilation is cached by source hash.

Results are recorded in `results.json`. These are correctness runs, not timing
benchmarks. Full native tables, equality and visibility are checked separately
from same-Run/match-history equality against the unoptimized GPU interpreter.
Static prefix availability on the saved full Pass-3 typed core is reported
separately from runtime reuse; it is not a hit-rate estimate.


## Subsequent execution upgrade

The batch path now uses multiple threadgroups for same-recipe context evaluation
and witness-checked successor effects inside Saturate. This file records the
prior region/cache baseline; its differential checker explicitly disables
composition to keep that baseline useful. Current implementation and acceptance
checks are documented in [staged batches](../staged_batches/README.md).
