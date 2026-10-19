# Complete captured raytrace Pass 3

The opt-in [stable invariant-choice implementation and new measurements](STABLE_CHOICE.md)
now pass full same-submit CPU/GPU graph equality and actual Tiger-output checks.
This page retains the original-policy benchmark and its failing graph gate.

This benchmark replays the real Pass-3 input saved after the preceding production
passes and their extractions. It executes the **entire last main schedule**, not
only one `terms` hotspot. It is a saved-input replay on the local modified native
kernel, not pristine egglog or a fresh three-pass frontend/Tiger-extraction run.
The native flattened schedule's semantics apply; its Repeat behavior is not a
replacement measurement for the production experimental scheduler. Do not combine
these seconds with the earlier production saturation-time profile.

The explicit GPU partition is `terms`, `terms-helpers`, and
`terms-helpers-helpers` (14 rules). Other rulesets execute natively at their original
schedule positions. Native committed row changes and unions feed the persistent
GPU mirror before the next assisted query. No rule is silently dropped because
its query syntax is unsupported.

Three lanes execute once each from the same initialized engine:

1. Native fused query/action execution of the original schedule.
2. CPU control: cached native queries and sorted native action submission for
   the selected 14 rules, native execution elsewhere.
3. Hybrid GPU: persistent GPU queries and the same sorted native action submission
   for those 14 rules, native execution elsewhere.

This separates the change in query backend from the existing change in commit
ordering. It **does not establish equivalence** merely by equal binding counts.
The full original fact/value/equality/visibility comparison remains mandatory.

## Whole-Cell sat skip admission

The current certificate accepts a fixed positive constructor/union environment
and a single direct ruleset saturation. This capture has function updates,
numeric guards, custom merges, and saturation over several interacting rulesets.
`Library::validate_environment` checks the existing proof contract without Bake
or candidate search. The report records its first unsupported declaration, and
the actual compiled queries' function reads/guards.

No valid whole-closure library is available here. Consequently no sat-skip hook
is installed: **0 certified/skipped boundaries; sat-skip timing is null**. We do
not time a fake hook that always declines and label it GPU + Cell. A first failure
on `LoopNumItersGuess` does not mean removing that declaration would certify the
terms subsystem: `ExtractedExpr :merge (Smaller old new)` also needs a proof.
Query compiler coverage is not Cell coverage.

## Reproduction

Use the same captured input, worker and current benchmark source. The input hash
and original capture metadata are recorded in `results/provenance.json`. The
capture is an untracked generated input; if missing, use the existing
eggcc-pass-benchmark capture/flatten workflow. Do not replace it with the original
raytrace IR before Pass 1.

```sh
mkdir -p out/cell-locality-worker
xcrun swiftc -O cell-assisted-egglog/matrix.swift -o out/cell-locality-worker/matrix
cargo build --offline --release --example raytrace_pass3_bench --target-dir target/analyzer

RAYON_NUM_THREADS=1 EGGLOG_PROBE_COUNTS_OFF=1 \
target/analyzer/release/examples/raytrace_pass3_bench \
  /Users/mineralsteins/Repos/egg_related/egg_layout/out/raytrace-interactive-20261010/pass-3-flat.egg \
  out/cell-locality-worker/matrix out/raytrace-pass3-new time
```

Output directories must be new. `time` disables query audits; `check` enables
them and is not a timing run. The command deliberately exits nonzero if the
same-commit final graph differs, **after saving the failed report**. Current full
Pass-3 results do fail that gate; see [RESULTS.md](RESULTS.md).

Optional separate diagnostics use the same arguments and a fresh output path:

- `audit`: run only the hybrid GPU schedule with an exact native incremental
  query comparison at each selected-rule frontier; save `audit.json`.
- `cpu-pair`: execute the CPU control twice from the same initial graph and
  compare full final graphs; save `cpu-pair.json`. Not an averaged benchmark.

All timing lanes exclude shared source parsing/schema/input initialization,
engine cloning, graph checks, and extraction. Worker/query setup is reported
separately. Compilation is not timed. Probe counters are disabled and the native
engine uses one worker thread. Run with no other benchmark/build in parallel.

## Correctness checks

`examples/support/live_table_bench.rs` compares final graphs modulo new ID
allocation, anchored to the common initial graph. Quality diagnostics also use
**initial Expr anchors**, canonicalized independently in each final graph before
reading `ExtractedExpr` and the complete `TCPair` cost set. Fresh numeric IDs
cannot be compared across executions. Costs do not certify identical extracted
Term syntax, output IR, or all facts/equalities.

```sh
CELL_MATRIX_WORKER="$PWD/out/cell-locality-worker/matrix" RAYON_NUM_THREADS=4 \
cargo test --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --test hybrid --test table_live --target-dir target/analyzer -- --test-threads=1

CELL_MATRIX_WORKER="$PWD/out/cell-locality-worker/matrix" RAYON_NUM_THREADS=4 \
cargo test --offline --test sat_skip --example raytrace_pass3_bench \
  --target-dir target/analyzer -- --test-threads=1
```

The small hybrid regression checks native arithmetic, subsumption, union and
subsequent GPU consumption in schedule order. Benchmark-helper tests cover ID
renaming, real equality/value differences, and input-anchored extraction costs.
These passing tests do not override the failing real-workload graph gate.
