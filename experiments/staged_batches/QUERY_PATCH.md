# Restore incremental query-index updates in the resident engine

The source compiler already supported journals for function output/timestamp
updates, but resident lowering disabled them. Thus each changing function value
forced the next stage to index the whole live graph again.

Resident lowering now enables the existing journal. Prepare appends new rows,
patches changed output/timestamp links, then clears old-value journal entries.
Moving an entry to a new bucket initializes its generation first. Row order is
preserved. Union compaction and subsume still request a complete index rebuild.
Generation-tagged heads are selected by serial resident preparation, independently
of whether query patching is enabled; parallel preparation retains its own format.

2026-10-30 actual Metal A/B diagnostic, same worker and `.egg` input:

| Measurement | Rebuild query index | Patch changed entries |
| --- | ---: | ---: |
| Whole index resets | 33 | 1 |
| GPU time | 332.05 ms | 65.91 ms |
| Inference time | 342.19 ms | 75.38 ms |

The input has 16,512 rows, final state 16,576 rows, and 66 Runs. A `min` function
decreases repeatedly; a later ruleset consumes a specific output value. All rows,
identities, visibility, per-Run fingerprints and matching/effect counters agree.
Only diagnostic reset/append counters differ. This is index-maintenance credit,
**not full Pass-3 performance or stronger Cell proof coverage**.

Validation also includes all 18 resident contract fixtures (17 successful repeated
executions plus expected native/GPU overflow), all four fresh-process checkpoint
cases and 30 compiler/IR/reproducibility tests. The union/merge/subsume cases ensure
patching never replaces the required reset after structural changes.

```sh
python3 experiments/staged_batches/check_query_patch.py \
  --build out/eggfuse-clean-paths-build --output out/query-patch-check
```

Results are in `out/resident-query-patch/result.json`,
`out/query-patch-all-contracts/results.json`, and `out/query-patch-checkpoints`.
