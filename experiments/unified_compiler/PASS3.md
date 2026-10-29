# Full raytrace Pass 3 through the unified compiler

Actual GPU-owned runtime, original complete saved input and schedule. The new
compiler entry now connects executable multi-stage Cell recipes to the existing
joint GPU program. This is not the single-context reference interpreter.

## Result

| Metric | Measured |
|---|---:|
| Complete GPU inference | 38.9557 s |
| GPU setup, outside inference | 74.9154 s |
| Accepted joint scopes / rejected scopes | 12 / 0 |
| Stage patches | 2,826 |
| Boundary recipe actions / fallback actions | 81,104 / 0 |
| Candidate-update recipe actions | 113,916 |
| Updates with an existing result | 42,713 |
| Guard witness pairs eliminated by exact summaries | 2,065,416 |
| Joint terms GPU time | 5.0156 s |
| Final GPU rows / classes | 699,784 / 524,742 |
| Production Tiger extraction | 2.0080 s |
| Extracted functions / selected rows | 29 / 8,019 |

Recipe actions need not be committed mutations. Joint scopes cover `terms`,
`terms-helpers`, and `terms-helpers-helpers`; the rest of Pass 3 uses its original
execution. There is no Ridge prediction in this path. The source-marker selector
selected **zero** markers from this training catalogue and its currently
supported fully-keyed spine domain. No benefit is credited to new source markers.

The historical boundary-recipe GPU run took 42.3360 s, but its worker differs.
It is retained as an endpoint reference, **not a fresh same-build speedup pair**.
No new 2x or marker-optimization claim follows from these times.

## Validation is deliberately not all green

- Original `source_sk` and generated `source_sk_mk` native endpoints agree on
  all original facts, values, equivalence and visibility: 697,922 rows.
- Full GPU endpoint equals the saved historical GPU endpoint, including complete
  equality/visibility. Typed inputs/rules agree; only redundant singleton `seq`
  wrappers differ in the source-printed control tree.
- **Native versus GPU full graph fails:** 697,922 versus 699,784 rows. This is the
  existing backend difference, reproduced unchanged by this pipeline. Most of
  the row-count difference is `Smaller` (+1,777); table deltas are in the JSON.
  Counts do not explain the causal mechanism or establish semantic equivalence.
- Fresh production Tiger output is byte-identical to the previously verified
  GPU selected DAG. The importer verifies all rows without running inference.
  This is extraction equality to the historical GPU result, not proof that the
  native/device full graph difference is harmless.
- The small real-rule training fixture passes the complete native/GPU pipeline:
  214 rows, 182 classes, one joint scope, 31 stage patches, 12 boundary actions,
  26 update actions (13 with an existing result).
- Six contract/cache tests pass. A real precompiled stage artifact loaded with
  ordinary host options is rejected before launching the GPU worker.

The pipeline retains failure exit status for the native mismatch. A completed
schedule is not reported as successful native verification.

## Bake and compilation

Full-input Analyze was attempted and terminated with SIGKILL during initialization
history processing; the cause was not established. The successful run explicitly
uses `out/cell-update-pass3/training.egg` as `--bake-source`: the saved small input
with the real Pass-3 rule environment, not simplified substitute rules. Typed
schema/merge and all nonempty training rulesets are checked against the full
execution source before analysis. The training run observes 74 applications and
141 logical matches. It is not a full-input coverage measurement.

Eight boundary anchors are admitted: Const, Arg, Bop, Top, Uop, Get, Single,
Concat. The exporter validates instructions against the actual marked program;
unknown cases retain the complete ordinary path. Full execution input/schedule
are never replaced by training facts/schedule.

Cold recorded pass costs: SK 11.02 s; environment validation 2.66 s; small Analyze
13.67 s; marker selection 2.51 s; typed marker IR 2.65 s; recipe export 0.12 s;
device lowering 0.71 s. Native references used the debug frontend and took
548.53 / 556.78 s; these are validation costs, not CPU performance baselines.

```sh
python3 -m tools.eggfuse \
  --source /Users/mineralsteins/Repos/egg_related/egg_layout/out/raytrace-interactive-20261010/pass-3-flat.egg \
  --bake-source out/cell-update-pass3/training.egg \
  --output out/unified-pass3-multistage \
  --build out/device-build-scheduled-reviewed \
  --solver-python /Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python \
  --capacity 1048576 --match-capacity 1048576 --multi-stage-cell

# Read completed inference/failed verification without running them again:
python3 -m tools.eggfuse.report out/unified-pass3-multistage \
  --reference out/cell-spines-pass3-full
python3 -m unittest tools.eggfuse.test_multistage tools.eggfuse.test_passes -v
```

Raw input SHA256:
`c727cdf724b020ae585c4051fda36b66576c1ce4252a4d9d59fbb7522d0c7d4f`.
Device source/program/binary fingerprints, complete checks, table deltas and
smoke receipt are in `pass3-results.json`. Generated artifacts remain under
`out/unified-pass3-multistage/eggfuse_related_files`; they include the executable
`multi-stage-cell.json`. Host execution flags and the recipe checksum are bound
to the precompiled kernel, preventing a silently inactive stage optimization.
