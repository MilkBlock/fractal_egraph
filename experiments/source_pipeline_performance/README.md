# Source-only pipeline: Raytrace Pass 3 regression

This measures the real complete saved raytrace Pass-3 workload with the unchanged
Metal worker and native RHS/merge/rebuild. It is not the small counter fixture.
Each timing lane runs once, without query audit or profiling counters. Reuse
validated lanes rather than re-running controls. Offline source profiling runs
separately from inference; its process was suspended during the first timing
pair, with the pause recorded explicitly.

The tested runtime is commit `4485acf`. This experiment changes benchmark
reporting only, not its key generation, plans, semantics or execution engine.

## Confirmed regression before marker insertion

| Same-binary lane | Inference | Original rows | GPU dispatches |
|---|---:|---:|---:|
| Original source, GPU, no implicit Cell service | 17.041271 s | 699,068 | 7,260 |
| Generated `source_sk.egg`, GPU | 35.214278 s | 699,068 | 7,260 |

The SK stage is **18.173007 s / 106.64% slower**. Exact original table rows,
function values, equality partitions and subsumption flags agree. All
2,458,173 observed original row events and 239,476 union events also agree.
This is extra maintenance work, not a larger inferred original graph.

The key stage creates maintenance for **94 sorts / 238 constructors** and runs
it before almost every original ruleset. Its measured native work is:

- `cell-stable-keys`: 23,677 Runs, 15.912116 s, 1,044,670 logical matches.
- `cell-stable-freeze`: one Run, 0.004144 s.
- Key search/apply: 13.354876 s (not a pure Join-only timer).
- Final auxiliary structural-key rows: **524,631**, raising all exported rows
  from 699,068 to 1,223,699. These are table rows, not a claim of new original
  e-nodes or committed mutations per logical match.

The 15.916260 s helper time is 87.6% of the observed 18.173007 s delta. This is a
phase accounting observation, not an assertion that timers are independently
additive or that all remaining time has a single cause.

**None of the 504 admitted source query plans reads a `CellStructuralKey-*`
atom.** The current later compiler does not use the newly materialized keys to
replace original matching. This is the immediate design error: maintenance was
expanded without adding useful consumers. Restoring table-name-specific choices
is not necessary to remove dead/unused auxiliary computation.

## Why the historical 13-second result is not a strict denominator

The previous runner also changed `to-hoist` selection and installed joint/scalar
closure services. The new source-only runner does neither implicitly. A saved
12.735386 s result has eleven additional proof-gate tables; even after projecting
those auxiliary tables, its equality partition differs at **`to-hoist`** from
the current original-source lane. The 13.324834 s whole-program-context result
is historical background, not an interchangeable same-semantics timing control.

The source-only cleanup therefore changed execution policy/services as well as
packaging. Its small correctness test was insufficient as a Pass-3 regression
gate. The same-policy 17.04 → 35.21 comparison above avoids that confound.

## Measurement boundaries

Inference excludes offline profiling/selection, native setup, shader compilation,
graph export and extraction. GPU setup was 28.885430 s for the raw lane and
2.734537 s for SK; these cold/warm setup observations must not be interpreted as
an execution optimization. Extraction quality was not independently measured in
this experiment; exact original graph agreement is the stated correctness check.

`program-modes` now accepts `--keys` to report all three source stages and their
checked timing deltas. Failed, unequal or audited lanes receive no ratio.
`structural-pipeline` records per-stage wall time, including failures. The
reproducible sources and full graphs remain under `out/source-only-pass3-*`.

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py structural-pipeline \
  --source INPUT.egg --output out/source-only-pass3 --no-build
python3 skills/cell-assisted-egglog-bench/scripts/bench.py program-modes \
  --backend gpu --source INPUT.egg \
  --keys out/source-only-pass3/source_sk.egg \
  --marked out/source-only-pass3/source_sk_mk.egg \
  --output out/source-only-pass3-comparison --worker MATRIX_WORKER --no-build
```

## Repair priorities

1. Derive the set of needed keys from actual consumers. Do not repeatedly saturate
   every possible key table when no subsequent query reads it.
2. Export the validated closure services explicitly in the source contract;
   removing hidden discovery should not silently remove the optimization itself.
3. Charge marker publishers, maintenance schedules and consumer execution in the
   same model. A reduction in GPU candidates does not establish a net gain.

No runtime optimization is applied during this measurement.

## Interrupted final-stage generation

The user redirected work to a generic key-pruning algorithm while the new SK
program was still being profiled. That child process was terminated and the
partial profile was preserved, explicitly excluded from training. Therefore
there is **no completed SK+marker Pass-3 timing** in this report. The two completed
runtime lanes and their same-graph regression remain valid. Do not describe the
interrupted generation as a compiler error or as a successfully timed pipeline.
