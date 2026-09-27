# Native allocation comparison against pristine egglog

This measures the **actual egglog runtime**, without tracing, RipSat analysis,
macros, virtual storage, graph export, or diagnostic sampling in the timed path.
It does not demonstrate that the adaptive RipSat runtime beats ordinary egglog.

The baseline is the immutable `egglog-baseline` tag
(`c7553a6c2b16c529c13f3eb7eecd742495b992d3`), checked out without source modifications.
Both versions execute the original `egglog/tests/math-microbenchmark.egg`, including
its original 11 rounds, using the same public-API harness. Both use identical
Cargo.lock, ThinLTO, one codegen unit and compiler. The standalone harness enables
`syn/full` for host proc-macro compatibility in **both** builds; the baseline itself
is not patched. Build time is excluded.

## Allocation change

Parallel table insertion previously reserved 262,144 rows and hash entries for
**every shard**, even empty shards. It now returns from empty shards and reserves
according to the first actual input batch. Subsequent growth remains ordinary
amortized container growth. The existing flush size, row processing, merges and
rule scheduling are unchanged. Empty insertion/deletion queues now return immediately
based on their published row counts, avoiding an unnecessary scan of all shards. No rule is skipped and no workload-specific
heuristic is introduced.

## Results

Fifteen independent processes per variant, rotated order, automatic Rayon threads.
All processes report 944,254 logical matches (not committed mutations).
Execution time includes engine construction, parsing and execution, but excludes
engine destruction. `through_drop_seconds` and external process times are also
recorded. RSS is external peak process RSS, including destruction.

| Allocator | Pristine execution | Optimized execution | Pristine peak RSS | Optimized peak RSS |
|---|---:|---:|---:|---:|
| System | 285.80 ms | 285.26 ms | 481.14 MiB | 368.66 MiB |
| mimalloc (original CLI allocator) | 173.85 ms | 168.93 ms | 581.56 MiB | 488.67 MiB |

These are the final combined-change measurements. With the original CLI allocator,
execution is 2.83% lower and peak memory is 15.97% lower. Paired execution times
favor the change in 13/15 repetitions. A deterministic paired-median bootstrap
(10,000 resamples, seed 42) gives a 95% interval of 2.04–8.52 ms saved, with a
paired median of 5.51 ms. This is a **small measured native-runtime win on this
workload**, not an end-to-end RipSat acceleration result or a universal guarantee.
System allocation shows essentially equal execution time and 23.38% lower RSS.

Earlier staging-only experiments are retained, not replaced by the favorable
final run. The initial seven-process experiment found less than 1% improvement;
the 15-process confirmation found 4.1% with System and 2.6% with mimalloc, but its
paired mimalloc bootstrap interval crossed zero. The subsequent empty-queue
ablation is in `results/empty-ablation.json`.

All observations are in `results/{initial,confirm,final}.json`, including
compiler/binary hashes, commands and individual timings. Results are machine
specific. External process times have coarser resolution; final mimalloc medians
were 0.19 s versus 0.18 s. Graph verification is excluded from timings.

A second experiment removed eager global row reservation on the serial path.
Its 12-run median (165.97 ms) was not better than the staging-only change
(165.15 ms); it was **reverted**. See `results/reserve-ablation.json`.

## Correctness

Separate untimed executions export all Math constructor facts from both engines.
The exact ground-graph verifier establishes a bijection between canonical eclasses
and checks every constructor key, decoded primitive argument and output class:
**1,047,896 e-nodes and 443,832 e-classes, exactly equal**. Count equality alone is
not used as proof. Graph JSON files are intentionally not tracked.

Core-relations: 56 tests passed. Relevant integration suites: 30 tests passed
(native virtual facts, adaptive runtime, interior RHS skipping, enclosure audit,
and single-pass analysis). The new staging regression covers duplicate-heavy
writes, empty shards, updates and fresh rows under one and four threads.

## Reproduction

Use an existing clean checkout at `egglog-baseline` as BASELINE. The build helper
checks its commit and source cleanliness; it never modifies that checkout.

```sh
python3 tools/bench/build_native_compare.py --baseline "$BASELINE"
python3 tools/bench/native_compare.py --output out/native-recheck --runs 15

# Exact verification, deliberately outside timed measurements:
out/native-compare/bin/pristine-mi egglog/tests/math-microbenchmark.egg out/original-graph.json
out/native-compare/bin/after-mi egglog/tests/math-microbenchmark.egg out/optimized-graph.json
PYTHONPATH=tools/bench python3 -c 'from math_graph_equal import verify; print(verify("out/original-graph.json", "out/optimized-graph.json"))'
```

Automatic threads are the default; `--threads 4` can be used for an explicitly
fixed-thread experiment. Do not compare different allocator/thread settings.
The measurement helper has an external 30-second/4-GiB guard; it does not alter
native matching or internal saturation semantics.
