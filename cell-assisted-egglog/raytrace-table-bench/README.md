# Benchmark the actual raytrace search hotspot

This reuses the production-derived Pass-3 checkpoint and original schedule from
[snapshot-bench](../snapshot-bench/README.md), rather than generating another
sparse synthetic consumer. The input was produced after the actual first two
passes and their Tiger extractions. `raytrace.rs` and captured source hashes are
checked before running. No new trace, analyze, bake or frontend run is needed.

The selected packet is `/3/repeat0/4` (packet 349):

```egg
(saturate
  (seq
    (run terms)
    (saturate
      (seq (run terms-helpers)
           (saturate (seq (run terms-helpers-helpers)))))))
```

It contains 14 unchanged rules: construct Terms from child candidates, propagate
`PotentialExtractedExpr`, update `ExtractedExpr`, and select `Smaller` winners.
These include real custom-function reads, comparisons, aliases, literal operands,
set/merge and union semantics. No guard is removed to make the queries supported.

## Two distinct measurements

1. **Whole original packet, native fork.** One release execution plus a separate
   diagnostic replay from the same in-memory checkpoint. The graph retains its
   original timestamps and schedule state; no JSON graph is re-imported. The two
   replays must agree on every canonical row/value/subsumption flag and every
   rule's match count. Search+apply must exceed insert/merge plus rebuild, or the
   harness rejects this workload as not search dominated. This timer includes
   RHS staging; pure RHS is not separately available in this native report.
2. **GPU search backend on a real state inside that packet.** After the original
   `terms` and `terms-helpers` execute once, compare all 14 complete LHS queries
   against native. This includes enabled cost-improvement bindings and negative
   tests. CPU receives no GPU-generated candidates; GPU receives no native full
   matches. Results are compared after their timers.

The second measurement uses the new binary table backend, with one original
atom per fragment table. It deliberately does **not** label raw rows as certified
Cells. This is a real-data backend query benchmark, not a Cell coverage result or
a claim that all 14 original RHS actions now run in the numerical GPU executor.
The normal consumer interface still requires its analyzed Cell certificate.

The query-only switch uses the same tuple-index, delta-join and exact i64 guard
kernels, but skips keyed proposal reduction. Its match record contains an unused
constant RHS field, never submitted as an effect. The original native packet
still executes its real RHS, merge and rebuild. The earlier experimental query
run unnecessarily reduced all results into one dummy key; it was discarded as a
matching performance comparison and replaced by the recorded query-only run.

## Timing boundaries and checks

- Native query time includes compilation of its exhaustive query, callback
  collection, and canonicalization. It is not a native semi-naive iteration.
- Cold GPU query time includes static plan compilation, one worker/shader setup
  per rule, original table scan/filter/deduplication, typed binding encoding,
  binary upload, index construction, matching/guards, download and reconstruction.
- `roundtrip_seconds` includes binary input/output and device dispatch. Device
  timestamps are a subset; there is no claim that an isolated kernel timer is
  end-to-end execution.
- Common graph cloning and native action-sink preparation are separately
  reported, excluded from both query times. Original RHS is not executed by a
  query; a regression checks that it cannot create new constructor rows.
- There is one execution per measurement, no averages. OS and Metal caches are
  not flushed. Fourteen cold query sessions do not mean fourteen launches per
  saturation iteration; a real integrated runtime would retain its sessions.
- Table data is duplicated across queries in this benchmark. Upload totals and
  source-table sizes show that cost; table/plan reuse is not assumed for free.
- All match sets must agree exactly, including zero-match rules. Repeated slot
  aliases, literals, subsumption and numeric guards have a focused GPU regression.
- `--gpu-check` is not used to supply candidates. The native query here is an
  independent result oracle and the CPU timing control.
- The native packet is the **current fork**, not pristine upstream. There is no
  complete GPU packet or extraction comparison. Do not divide one-query-snapshot
  time into the full packet's 1,017 iterations to invent an acceleration factor.

The production skill's saved 2026-10-07 raytrace report separately measures
Pass-3 search/binding (18.882 s), RHS (0.406 s), insert/merge (0.319 s), and rebuild
(3.869 s). Those are historical full-pass measurements. They motivate selecting
this family; none is substituted for the fresh numbers in [RESULTS.md](RESULTS.md).

## Reproduce

From the `codex/cell-assisted-egglog` worktree:

```sh
mkdir -p out/raytrace-table-worker
xcrun swiftc -O cell-assisted-egglog/matrix.swift \
  -o out/raytrace-table-worker/matrix-metal

python3 cell-assisted-egglog/bench_snapshot.py \
  --output out/raytrace-table-search-new \
  --table-worker out/raytrace-table-worker/matrix-metal \
  --pre-query-run '(seq terms terms-helpers)'

python3 cell-assisted-egglog/raytrace-table-bench/summarize.py \
  out/raytrace-table-search-new/report.json \
  --output out/raytrace-table-search-new/summary
```

The helper builds the release REPL with `native-read-profile`; `--skip-build`
reuses a known current binary. The default capture is the retained production
`out/raytrace-interactive-20261010/pass-3-flat.egg` in the main checkout. Override
`--snapshot`, `--capture-metadata`, and `--breakpoint` together for a different
captured workload. Use a new output directory. To regenerate the capture, reuse
the existing `eggcc-pass-benchmark` skill and flattening workflow.

The REPL command is `:bench-tables NAME MATRIX_WORKER`; it does not advance the
schedule cursor. `:bench NAME` measures the intact native packet separately.

```sh
CELL_MATRIX_WORKER="$PWD/out/raytrace-table-worker/matrix-metal" \
  cargo test --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted --test tables --test bench_snapshot
cargo test --offline --release --example rule_repl --features native-read-profile \
  --target-dir target/analyzer -- --test-threads=1
```

Recorded raw artifacts: `out/raytrace-table-search-verified/`. The checked-in
`results.json` preserves provenance, exact rules, query/table counters, complete
native packet timings and equality checks. `queries.csv` is the sortable table.
No GPU Cell coverage is inferred from node coverage or grammar admission.
