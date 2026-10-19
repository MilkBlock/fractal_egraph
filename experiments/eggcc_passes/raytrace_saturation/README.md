# How much raytrace time is inside Saturate?

Measured on 2026-10-19, Apple M3 Max, production `check_statewalk@ae80f82a`,
egglog `ebba7bb` / egglog-experimental `08771f9`, default parallel schedule,
single native worker. One accepted timed three-pass run, no averages. Extraction
from each pass is the real input of the next pass's new egraph. No analyze,
coverage replay, Bake, GPU execution, per-match tracing or Graphviz rendering.

**39.024 s / 39.768 s = 98.13% of scheduled execution is inside Saturate.**
It is 97.48% of pass execution including setup, and about 76.83% of the complete
50.793 s command. Extraction takes 7.888 s and serialization another 2.000 s.
See [RESULTS.md](RESULTS.md) for per-pass denominators and hottest blocks.

The largest two saturated blocks are nested `terms` (16.374 s) and
type/index/always-run helpers (14.933 s). This establishes where runtime is
spent, **not** that the present positive constructor/union Cell certificates can
replace all these blocks. Custom merges, primitives, subsume and nested schedule
ordering still need separate correctness arguments.

## Measurement semantics

`schedule_profile.rs` is included only in a temporary copy of the production pass
driver. The build script restores the original source and executable afterwards;
it does not modify the engine or Cargo dependency sources. The profiler copies
its standalone executable to `out/`. Kernel counters already present in RunReport
provide search+RHS, insert/merge, rebuild and logical match totals. RHS is **not**
independently instrumented in this mode.

- Measure the outermost `Saturate` as one original native command, including
  every nested schedule. Never add its children's time again.
- Expose only outer Sequence/Repeat boundaries. Ordinary `Run` outside any
  saturation gets its own timer. Rule installation, data initialization actions,
  parsing and typechecking outside these commands remain separate.
- Native `Schedule::Repeat` uses AND-of-`can_stop` early exit. Production's
  experimental `run-schedule` is a **UserDefined command** whose `repeat` runs
  every requested iteration. The profiler preserves both implementations.
- Extended `saturate` itself still executes through `RunExtendedSchedule`;
  it is never substituted by the core Saturate implementation. Scheduler-scope
  extensions (`let-scheduler`/`run-with`) are explicitly rejected by the profiler.
- Per-command wall time includes its native preparation and dispatch. Formatting
  and aggregation happen outside the command timer but inside pass execution.
- The sums of packet search/RHS, merge, rebuild and logical matches must exactly
  account for native overall totals (floating timers within tolerance). This
  prevents hidden UserDefined schedules being misreported as initialization.

The first profiling attempt missed the experimental command and reported no
Saturate time; its timing breakdown is invalid and excluded. It ran the complete
experimental schedule intact, so its semantic output is retained as a control:
all three pass sources, node/e-class counts, logical match totals, and the final
optimized program are identical to the corrected run. This is **not** an exact
full-egraph isomorphism comparison. The corrected production binary additionally
passes five small extended-scheduler comparisons (repeat, nesting, until, every
relation fact and per-rule match counts). The local test covers five ordinary
native schedule cases. Production self-tests run once before pass timing.

Raw accepted packet records and hashes are in `results/`. Complete command wall
includes frontend and final pretty-printing; this recorded run also includes
the small post-command source/binary fingerprint cost. No rendering is timed.
Production driver/source restoration was verified clean after the measurement.

## Reproduce

Use Python 3.9+ and a fresh output directory. Build the schedule profiler once;
later measurements can reuse that binary. From the egg_layout worktree:

```sh
/opt/homebrew/bin/python3 experiments/eggcc_passes/build_profile.py \
  --schedule-only \
  --production-repo /Users/mineralsteins/Repos/egg_related/check_statewalk \
  --output out/eggcc-saturate-profiler-new

/opt/homebrew/bin/python3 experiments/eggcc_passes/run.py \
  --timing-only \
  --production-repo /Users/mineralsteins/Repos/egg_related/check_statewalk \
  --profile-binary out/eggcc-saturate-profiler-new/eggcc-schedule-profile \
  --source /Users/mineralsteins/Repos/egg_related/check_statewalk/benchmarks/passing/raytrace/raytrace.rs \
  --output out/raytrace-saturate-new

# Recompute tables from saved raw measurements, no new compilation:
/opt/homebrew/bin/python3 experiments/eggcc_passes/saturation_report.py \
  experiments/eggcc_passes/raytrace_saturation/results

cargo test --offline --test schedule_profile --target-dir target/analyzer
```

Do not run concurrent builds/edits in the production checkout while preparing
the instrumented driver. No production dependency cleanup/rebuild is requested
by the schedule-only script; Cargo recompiles affected crates normally.
