# Same-result release benchmark

This harness compares the immutable **pristine `egglog-baseline` engine**, the
current native fork, and the actual marker/matrix submission runtime. It does not
infer speedup from fewer matrix operations or compare checked GPU execution with
unchecked native execution.

The measured suite has two controlled consumers using an existing analyzed
eggcc Single/Get Cell. `local` joins three table atoms with an i64 guard and min
update. `sparse-cycle` joins six table atoms: three single-successor links must
close onto the Offer's destination; only every 31st path closes. Its extra rules
and facts are generated test workloads, **not a complete production eggcc pass**.
No compiler extraction is present; exact graph and final function values are the
quality criterion. Native rule matching remains fast even at the larger sizes.

## Validity gates

- Release ThinLTO/codegen-units=1, System allocator, pinned registry versions.
  The same public-API Rust harness builds against pristine and current kernels.
  The pristine checkout must match `egglog-baseline^{}` and have a clean kernel.
  Neither the baseline checkout nor its immutable tag is modified.
- Each size uses identical rule environment/input files and four driver rounds.
  Every timed path must end at a quiet boundary. Assisted scheduling is not
  asserted to have the same intermediate round states as native scheduling.
- Timed GPU processes have both `--gpu-check off` and `--audit false`, no graph
  export and summary-only stdout. Graph snapshots are now skipped entirely when
  `--dump` is absent, rather than computed and discarded after the timer.
- A separate fresh process enables strict pre-commit GPU checking and exports
  the graph. Every variant must equal pristine on typed facts, equivalence,
  primitive outputs and exact per-row visibility. Independently computed Best
  values and the expected effective binding/update counts are also checked.
- `test_contract.py` intentionally changes values, equivalence and visibility,
  enables audits/exports, and overlaps timers; the harness must reject these.
- Failed or incomplete suites cannot be summarized as speedup reports. Binary
  hashes are checked before execution. Input, environment, library, compiler,
  harness and source hashes are retained.

Timing is one fresh process per point, with no averages or confidence interval.
OS and Metal compilation caches are **not flushed**. Native sub-millisecond run
times and small differences between fork/pristine are not stable regression
claims. Graph validation is a separate replay, not an inspection of the timed
process's final heap. Timed and verified effective-work counters must agree.

## Timing boundaries

- `setup_seconds`: analysis/library loading where applicable, rule compilation,
  engine/session creation and worker startup.
- `input_seconds`: input parsing and initial fact construction.
- `run_seconds`: all rounds, including native helpers/events, Cell admission,
  marker indexes, candidate preparation, solver, shader/buffer installation,
  IPC, GPU work and native commits.
- `core_seconds`: setup + input + run. Nothing is subtracted from this number.
- `process_wall_seconds`: launch to process exit, including output and teardown.
- `phase_seconds`: disjoint named portions of run; uninstrumented gaps remain.
  `device_seconds` is a **subset** of dispatch time, not an additive phase.
- `install_breakdown`: plan validation, solver invocation, lowering and worker
  installation. Worker decode/buffer/compile/inherit times are **subsets** of
  worker installation. JSON decoding includes the current dictionary-to-JSON
  reserialization and typed decoding; transport includes further host work.

RSS is sampled every 10 ms across the main process and its live descendants by
an independent sampler. Process waiting does not use that polling interval.
These sums can miss short peaks and double-count shared pages; they are not a
unique physical-memory peak or GPU memory usage. Library file bytes are reported
separately. The existing Bake artifact is reused; offline Bake cost is not timed.

The original Cartesian assistant is not a native baseline. `--legacy-small`
adds only a fixed **local/16** diagnostic. During initial harness bring-up, the
six-table Cartesian/16 run was stopped at 7,688,208,384 bytes observed main RSS;
its full product has 50,331,648 combinations. It produced no accepted performance
result. The indexed variants and pristine engine have no such product materialized.
This restriction selects the benchmark workload, not a runtime candidate budget.
After the final measurements the flag was tightened to require local/16
explicitly; every measured legacy run already used that configuration. The raw
report retains the measurement-time harness hash; its exact script is retained
beside that report as `harness-used.py`. Timed executable hashes are unchanged.

## Reproduce

Build helper needs Python 3.11+; the run/test helper needs `psutil`. Both solver
backends use the existing `tools/cell_spine` environment; the large suite uses
SCIP so it does not depend on a restricted Gurobi license. No package download is
performed by these commands.

```sh
/opt/homebrew/bin/python3 cell-assisted-egglog/bench/build.py \
  --baseline /Users/mineralsteins/.codex/worktrees/native-baseline/egg_layout \
  --output out/cell-bench-build

python3 cell-assisted-egglog/bench/test_contract.py

python3 cell-assisted-egglog/bench/run.py \
  --build out/cell-bench-build/build.json \
  --output out/cell-bench-run \
  --layout-python /Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python \
  --sizes 16,64,256,1024,4096 --legacy-small

python3 cell-assisted-egglog/bench/summarize.py out/cell-bench-run/report.json \
  --output out/cell-bench-run/summary --plot
```

Outputs include raw timed/verification logs, graph comparisons, a full report,
compact JSON/CSV/Markdown results and optional matplotlib PNG/SVG plots.
Use a new run directory. `--cases sparse-cycle --sizes 1024,4096` selects a subset;
it changes the workloads, never truncates candidate matches.

## Earlier baseline findings

See [RESULTS.md](RESULTS.md) and `results.json`. All 42 final variant/case pairs
passed verification. Current GPU assistance is slower than pristine in this
suite. At 4,096 local instances the paired run spends about 4.16 s routing marker
bindings and 4.67 s decoding worker JSON, versus 30.8 ms of GPU device work.
On the sparse-cycle case, worker JSON decode takes about 9.24 s and device work
2.61 ms. Warm compiler caches make shader compilation a small part of these
particular runs; earlier cold-shape measurements were larger.

The solver is also real runtime cost: one local/1,024 run spends about 36.6 s in
planner invocation. The current model solves instance-level condition graphs
again when their topology grows; lower operation counts do not pay for this yet.
The next targets suggested by the measurements are binary/incremental program
transport, reusing compiled kernels/layouts, avoiding disconnected marker-prefix
enumeration, and compiling repeated layout families rather than repeatedly
solving expanded instance graphs. These optimizations are not claimed here.

## Binary table follow-up

The harness also supports `device-tables`, selected by
`--marker-layout device --matrix-layout tables`. Its static compilation happens
in setup; run includes table deltas, GPU joins, guards/reduction and native commits.
The ordinary paired control now uses binary heap transport too. This isolates
the additional benefit of eliminating CPU combination routing and expanded plans.
The archived results above predate both changes and remain historical evidence.

Select variants explicitly to avoid repeating the expensive old online solver:

```sh
python3 cell-assisted-egglog/bench/run.py \
  --build out/cell-bench-build/build.json --output out/cell-table-bench \
  --layout-python /Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python \
  --sizes 16,256,4096 --variants pristine,fork,marker-paired,device-tables
```

`--layout-python` is retained by the shared harness, but the table backend does
not invoke Python or any solver. The summarizer accepts the selected variants
and retains table upload/download bytes, index visits and resident match counts.
See [new results](../table-runtime/RESULTS.md). The same pristine graph/value gates
apply to every new variant; unsupported or failed cases do not become speedups.
