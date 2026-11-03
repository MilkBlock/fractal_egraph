# CPU optimization, 2026-10-31

This measures the **standalone Cell CPU executor**, not the older native-backed
CPU worker, pristine egglog, or Metal. It is still single-threaded. The baseline
implementation is preserved in commit `74e6c50`; no benchmark changes the source
rules or schedule. These are query/storage execution improvements, **not evidence
of automatic multi-rule closure skipping**.

## Complete Raytrace Pass 3: same-binary ablation

Both runs use binary SHA256
`0a44c8d67d42415f4354ca9a62637c1a61105014223b83eed6449cd3c168f562`
and program SHA256
`7acc3533b2be7f5da674867947ee49a4ea7141b46eda6e048ef4021d576da116`.
One clean process per configuration, no profiling/audit/concurrent compilation.

| Measure | Rebuild changed tables' query indexes | Update affected fact references |
|---|---:|---:|
| Inference | 206.784 s | **79.224 s (2.610×)** |
| Search, including lazy index builds | 166.274 s | 50.611 s |
| RHS | 8.769 s | 8.923 s |
| Alias/key repair | 6.019 s | 6.172 s |
| Index publication/maintenance | 15.538 s | 3.461 s |
| Event delivery and GC | 10.130 s | 10.018 s |
| Whole process, including IR parse and state export | 209.044 s | 81.533 s |
| Process peak RSS | 2.14 GiB | 2.36 GiB |
| Index builds | 9,630 | 65 |
| Facts decoded for initial/index rebuilds | 145,991,649 | 107,705 |
| Facts decoded for incremental index updates | 0 | 2,916,095 |

Both execute **11,057 Runs**, report **4,084,636 logical matches**, and produce
**702,159 visible/subsumed records** (661,151 virtual constructor/relation facts
and 41,008 live function ports). Logical matches are not committed mutations.
Final typed rows, values, equivalence identities and visibility are identical.
Every Run's path, iteration cursor, epoch, match count, changed flag and live
frame count is identical. This is not a whole-native-equivalence claim.

The compact incremental index consumes more memory than invalidating its entire
table; its gain is not memory compression. It stores per-column fingerprints and
fact references. Every candidate still undergoes full typed binding checks.
Union/updates/delete/subsume feed index changes at the Run publication boundary.

The earlier 192.575 s Raytrace run included query timing, progress logging and
sampling. It only diagnosed hotspots and **is not the timing control above**.
Interrupted initial/reverse-index Raytrace attempts are also excluded.

## Small frozen Pass-3 suite: cumulative changes

| Input | Initial CPU runtime | Final CPU runtime | Speedup |
|---|---:|---:|---:|
| fact | 127.547 ms | 28.675 ms | 4.45× |
| loopfact | 111.010 ms | 27.338 ms | 4.06× |
| sum-bits | 7,471.996 ms | 187.924 ms | 39.76× |

These controls use the earlier binary, SHA256 recorded in `results/final.json`.
They establish cumulative improvements, not the same-binary index ablation.
Independent iterations of the implementation are recorded in `results/` without
selecting the fastest run or averaging. Each timing was rerun only after changes.

The changes are delta-first joins, operator membership indexes, compact
incremental column indexes, reversible dense binding slots, shared immutable
query/RHS code, and operator-specific dirty-key repair. `sum-bits` candidate
visits fell from 21,981,402 to 187,489 with unchanged logical/effect work.

## Output memory and validation

The pre-streaming Raytrace run peaked at 8.22 GiB because it built complete JSON
graph and checkpoint images after inference. Streaming final state and making
completed checkpoints explicit (`--save-checkpoint`) reduced the process peak
to 2.36 GiB. This comparison changes output policy, so it is **not an inference
memory compression ratio**. Paused runs still write resumable checkpoints.

- 19 native semantic regressions pass, including late deltas, simultaneous
  deltas, alias-enabled joins, mutable function ports, custom merge constructors,
  non-idempotent primitive merges, containers, deletion, subsumption, bounded
  Repeat and until checks.
- The same 19 cases also pass with every query fingerprint forced to collide.
- Four checkpoint/resume cases and corrupt-checksum rejection pass; completed
  explicit checkpoint export was checked separately.
- Three IR integrity tests pass. The existing SK/marker/recipe artifact executes
  and passes its complete-LHS audit and independent-process replay.
- Before/after full per-stage small-suite audit is recorded in
  `results/full-stage-audit.json` (not used for timings).

The **pre-existing native differences remain unresolved**: extra Smaller rows in
all three small Pass-3 cases, plus loopfact equivalence and sum-bits TermSingle
differences. Native full-graph equivalence, production Tiger extraction quality,
and a speedup over pristine egglog are not claimed.

## What still limits this CPU backend

Search is still approximately 64% of complete Raytrace inference. The next
questions are how much of that work is repeated binding construction versus
essential alias alternatives, and whether matching can be parallelized without
duplicating indexes or changing ordered RHS effects. Event delivery/GC consumes
another 10 seconds. This is **not the CPU limit** and not yet a complete
multi-rule RipSat closure executor.

Reproduce the same-binary ablation with the existing suite runner:

```sh
CELL_CPU_QUERY_INDEX_SCOPE=table python3 experiments/cell_runtime/suite.py \
  --freeze --cases raytrace --output out/cell-raytrace-control
python3 experiments/cell_runtime/suite.py --cases raytrace \
  --baseline out/cell-raytrace-control --output out/cell-raytrace-incremental
```

Saved numerical evidence: `results/raytrace.json`, `results/final.json` and the
regression/audit reports. Generated graphs, binaries and checkpoints stay in
`out/cell-cpu-optimization/` and are not committed.
