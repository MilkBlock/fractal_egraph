# Cost of constructing engines in the coarse/CS saturation-equality phase

This profiles actual native egglog ripen jobs selected by the online CS/CCSS
pipeline, followed by its existing saturated-state catalog. It does not measure a
standalone mock graph or use intermediate-state convergence. Existing exact-source
cache reuse remains enabled: cached jobs are not charged the original donor's
ripen timings again.

Reproduce from the repository root:

```sh
cargo build --release --offline
python3 experiments/ripen_cost/measure.py out/my-ripen-cost
```

Default: one discarded warm-up plus five measured fresh-process runs per workload,
release mode on the local macOS host. Tier0 rounds are 5 for recursive-cs and 4 for
math-microbenchmark. Ripen has 8 rounds/job, 16 jobs total, 16 jobs/boundary and a
100000 ms between-job scheduling budget. No raw trace files are requested. Output
paths must be fresh. Source and instrumented-code hashes are in `results.json`.
Unrelated preexisting worktree edits were preserved, so provenance includes source
hashes rather than claiming a pristine checkout benchmark.

## Results

Times below are sums within a run, then medians over five runs. Percentages are
medians of per-run ratios, so dividing rounded median times may differ slightly.

| Quantity | recursive-cs | math-microbenchmark, 4 rounds |
|---|---:|---:|
| Timed CS/ripen/catalog phase | 72.97 ms | 146.29 ms |
| Saturated candidate jobs | 9 | 16 |
| Exact-source cache hits | 2 | 10 |
| Actual native ripen executions | 7 | 6 |
| Main ripen EGraph::default | 0.44 ms / 0.59% | 0.38 ms / 0.25% |
| Parse + normalize entry/rules | 0.92 ms / 1.25% | 2.32 ms / 1.54% |
| Install declarations/rules/entry actions | 4.80 ms / 6.48% | 10.76 ms / 7.11% |
| Main bootstrap (three preceding rows) | about 8.43% | about 8.92% |
| Native rule sweeps (with trace enabled) | 0.36 ms / 0.50% | 1.10 ms / 0.74% |
| Catalog, including reads/writes/grouping | 16.30 ms / 22.34% | 17.10 ms / 11.69% |
| Exact graph comparisons within catalog | 0.41 ms / 0.58% | 1.59 ms / 1.08% |

A main empty engine costs about 62–63 microseconds per actual ripen. Restricting the
denominator to native ripen calls (rather than the entire CS pipeline), main
bootstrap is about 28.9% / 45.1%. Thus 'empty engine construction' and 'preparing a
ready-to-run local problem' are substantially different costs.

There are also helper engines used for entry preparation/validation, parsing
export metadata and table statistics. Instrumenting all explicit EGraph::default
calls in native_ripen.rs and native_ripen_entry.rs gives:

| All instrumented engine constructions | recursive-cs | math4 |
|---|---:|---:|
| Count | 66 | 98 |
| Constructor time | 3.87 ms | 5.74 ms |
| Fraction of phase | 5.38% | 3.86% |

These helper-construction times overlap the enclosing preparation/export timings;
DO NOT add them as another disjoint stage. This counter does not include arbitrary
engine creation elsewhere in the program or tier0's own initialization.

The combined job time minus native ripen calls accounts for about 40.0% / 51.8%
of the phase. It includes entry construction, symbolic validation, cache handling,
files and promotion; the instrumentation does not attribute that remainder wholly
to any one operation. Catalog time is also much larger than exact comparison time.
Those observations argue against treating EGraph::default alone as the bottleneck.
A shared compiled environment might additionally save rule installation/validation,
but these measurements do not establish the speedup or correctness of such a design.

## Scope and counters

`queue.json.profile` accumulates across capture boundaries. `phase_seconds` covers
Pipeline::step through catalog/report preparation, excluding the final queue write,
outer tier0 execution and subsequent viewer/frame export. It is not whole-command
wall time. `jobs_seconds` includes cached and rejected work; stage-specific native
ripen totals only include successful uncached calls. These workloads had no rejected
jobs. Math4 ends with 175 Pending candidates: this is a bounded 16-job experiment,
not completion of all candidate saturations or the full math optimizer.

`ripen.json.timings` separates main construction, parse/normalization, setup,
native sweeps, trace import, Tier1 update, checks, state export and table statistics.
Setup intentionally includes seed actions; it is not a pure compile-only timer.
`catalog.json.timings` separates input read, key building and actual exact comparison.
The pipeline's catalog wall timer additionally includes serialization/DOT/output.
The catalog is rebuilt at successive boundaries, so its 8 / 36 recorded comparisons
are cumulative operations, not necessarily distinct pairs.

Instrumentation uses monotonic clocks and reports small stages without subtracting
clock overhead. Precision does not imply universal timings. No performance tests
run concurrently with the measurements. Tests were run afterwards.
