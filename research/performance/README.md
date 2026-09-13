# Capture performance on Apple M3 Max

All measured executables used release builds. Native baselines are medians of
three runs; pipeline variants are single runs with macOS `sample` and `ps`
monitoring overhead, so they are diagnostic rather than precision benchmarks.
Exact observations are in `capture.json`.

| Case | Time |
|---|---:|
| Native execution, 11 rounds, no trace | about 0.30 s |
| Native execution, 6 rounds, no trace | about 0.006 s |
| Pipe-based analysis, 6 rounds, initial default thread pool | 35.62 s |
| Single-threaded tier-1 import | 26.59 s |
| Only resolve bindings requested by each occurrence | 19.64 s |
| Indexed import, query shadow-check fix, shared cost extraction | 10.70 s |

These are different workloads: six-round analysis includes another egraph,
verification, extraction, fixture checks and rendering. It is not legitimate to
present its ratio to eleven-round native execution as a tracing-only overhead.

The dominant initial stage was tier-1 import (28.93 s). Samples visited
`eval_actions -> run_rules -> merge_all`, thread wake/wait calls, and free-join
materialization. Restricting routes reduced LocalArgs from 28,718 to 6,663 while
preserving 1,529 actual Bindings. A late isolated import sample identified
`Names::check_shadowing -> HashMap::clone -> String::clone`; query validation now
checks global names read-only. Rule-local scope checks retain their old behavior.

Split timing subsequently found 6.71 s in export alone: calling
`extract_value_to_string` per binding recomputed global extraction costs each
time. Sharing one extractor while the graph is immutable reduced that section
to 0.079 s without dropping the exported binding text.

A bounded eleven-round probe reached approximately 944,000 match events, then
hit the supervisor's 8 GiB process-tree RSS threshold at 19.49 s, still in the
trace/collection phase. It was deliberately stopped; it did NOT complete the
analysis. The pipe transport still retains a full typed trace and JSON object
in memory. This is not a bounded-memory online collector.

Disk fix: capture no longer writes profile.json, manifest.json, imported.egg,
native.json or interfaces.egg. It streams commands through bounded parser
batches and materializes tier-0 combinations only for visible fractal contexts.
A six-round output shrank from 31.07 MiB to about 7.98 MiB. Persistent tier-1
models can still grow; no constant-space claim is made.

Reproduce diagnostic profiling (prebuild release executables to exclude compilation):

```sh
CARGO_INCREMENTAL=0 cargo build --release --bin egg_layout
CARGO_INCREMENTAL=0 cargo build --release --manifest-path research/Cargo.toml --bin run_egg --bin combine_profile --bin tier1_export --bin tier1_interface_snapshot
python3 research/profile_capture.py --rounds 6 --output out/profile-new
python3 research/profile_capture.py --rounds 11 --timeout 60 --max-rss-mib 8192 --output out/profile-bounded-new
```

The next architectural step is direct in-process construction of tier-1 from
native events, eliminating whole-trace JSON and textual process hand-offs.
