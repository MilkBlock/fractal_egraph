# Resident Run checkpoints

This is the actual ABI7 Metal engine. A GPU gate suspends at an exact absolute
count of completed logical Runs, even with a larger queued host quantum. Pending
union/rebuild continuations finish first. A pause is not saturation or successful
completion of the source schedule.

Snapshots include all mutable buffers: rows, indexes, union state, identities,
watermarks, bindings, completion certificates, marker cache, stage stack, history
and scratch. Sizes and growth state are restored. No prefix rematching or RHS
replay occurs. Immutable bytecode comes from the pinned program. Buffer checksums,
program/worker SHA-256 and device identity are checked before GPU execution.
Changed shaders/binaries are deliberately rejected.

Writes stream in 8 MiB chunks. A complete directory is renamed into place after
all buffers and the manifest are written. Interrupted writes leave a temporary
directory, never a valid checkpoint. Storage is proportional to allocated capacity,
not live rows; this is diagnostic storage, not Cell compression. IO, compilation
and export are excluded from the reported suffix inference time.

```sh
# PROGRAM is a saved program-*/attempt-*/program.json from the current compiler.
python3 -m eggfuse replay --program "$PROGRAM" --build "$BUILD" \
  --output out/before-hotspot --stop-after-runs 100
python3 -m eggfuse replay --program "$PROGRAM" --build "$BUILD" \
  --resume out/before-hotspot/checkpoint \
  --output out/one-hotspot --stop-after-runs 101
python3 -m eggfuse replay --program "$PROGRAM" --build "$BUILD" \
  --resume out/one-hotspot/checkpoint --output out/remaining
```

Fresh-process checks, 2026-10-30 (`out/resident-checkpoint-v2`):

| Fixture | Runs | Suspend after |
| --- | ---: | --- |
| Three-stage chain | 7 | 0, 1, 2 |
| Union, custom merge, subsume | 4 | 0, 1, 2 |
| Context/history buffer growth | 20 | 0, 1, 17 |
| In-Run union continuation | 6 | 0, 1, 2 |

All match uninterrupted execution exactly for every Run fingerprint/control record,
final rows/IDs/values/visibility, effect counters and index maintenance counters.
Quantum changes across resumes. Changed-program and corrupted-buffer negative
checks reject before dispatch. The existing 30 compiler, IR and reproducibility
tests pass. No new full Raytrace speedup is claimed.

```sh
python3 experiments/staged_batches/check_checkpoint.py \
  --build out/eggfuse-clean-paths-build --output out/checkpoint-check
```

## Frozen executor images

New checkpoints also save the executed worker bytes and, when available, its
materialized Swift source. Resume verifies and uses that exact image; it does
not rebuild from a possibly changed worktree. The Metal pipeline archive remains
in the requested build cache, avoiding recompilation merely because the worker
image is loaded from a checkpoint directory. Diagnostic instrumented snapshots
are explicitly rejected as ordinary continuation points.

The regression resumes with a nonexistent current build and checks rejection of
a corrupted worker image. This is independent of source/program and state-buffer
checksum tests. Exact-Run diagnostics still pin ordinary schedule execution,
including after resume; they are not timing evidence for private region fusion.
