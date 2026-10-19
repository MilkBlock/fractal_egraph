# Does a better Bake put most matching work into known Cell contexts?

The original measurements below use dense row × projection reservation.
[Sparse-storage follow-up](../sparse_cell_storage/RESULTS.md) reruns the exact
same libraries/workload without reducing Bake: rich-library persistent GPU
buffers fall from 729.45 to 40.45 MiB, with unchanged matching/coverage.

A controlled experiment: **keep the test facts, eight consumers, execution and
boundary perturbations fixed; change only the frozen library**. All bodies are
produced by `whole_cell::Library::bake`, using native helper saturation followed
by independent delta replay verification. No closed state is handwritten.
`export_projection_catalog` exports these verified bodies for the existing GPU
positive-projection reader; it does not add an unchecked macro loader.

## Training and held-out workload

`helpers.egg` has the positive tuple helpers:

```egg
Single(x) -> Get(Single(x), 0) = x
Pair(x,y) -> Get(Pair(x,y), 0) = x; Get(Pair(x,y), 1) = y
```

`training.json` lists four symbolic coarse contexts:

```text
Single(x)
Pair(x,y)
Single(Pair(x,y))
Pair(Single(x),y)
```

Libraries are: none, Single only, the same Single context repeated eight times,
Single + Pair, and all four contexts. Repeated equivalent training is interned
into the same native-certified state. Its trigger/proof records remain distinct.

The fixed test contains all four structural families, each with fresh literal
identities. Two consumers per family have different numerical guards/RHS. Each
has three external relation atoms (`Boundary`, `Offer`, `Allowed`) plus either
two or five internal tuple/Get atoms. The eight LHS lengths are 5,5,5,5,8,8,8,8.
The library never contains those external relations or consumer guards/effects.

This is a structural coverage ablation, **not** a claim of generalization to
unseen structural families or a full eggcc benchmark. Bindings and external
facts are held out; the choice of families is deliberately controlled.

After an initial consume pass, all libraries receive the same new Boundary
facts. In a second phase, 1/8 of each family's roots lose a required Get through
subsume, and a different 1/8 gain an alternative constructor through union.
Helpers are not rerun after these perturbations. Missing/ambiguous contexts must
not be called valid solely because they appeared during Bake.

## What is measured

- Ready partial bindings / **all valid delta seeds**, before rejection or
  delegation. The denominator is kept comparable across libraries.
- Share of complete successful bindings produced by Ready work.
- Atoms covered by the located Cell context, separately from actual skipped
  internal reads. Ready means a usable context, **not an entirely internal LHS**.
- Ordinary join probes + decomposition locator probes. Locator work is not free.
- Bake time, initialization time, inference time, Cell projection count and live
  GPU buffer allocations. GPU allocations are not process RSS.

`intact` is the primary quality comparison. `disrupted` tests how far the same
library remains useful after boundary changes. Exact full-table values, equality
and visibility agree with a separate ordinary native execution at both phases.
Check mode additionally audits every assisted incremental frontier before RHS.

Timing runs have no atomic profiling or frontier audit. Each variant executes
once; no average is taken. Inference times exclude setup, input insertion, Bake
and correctness comparison. They include actual GPU runtime/transfer/native RHS
work, including capacity growth, so they are not steady-state throughput claims.

## Reproduce

```sh
mkdir -p out/cell-locality-worker
xcrun swiftc -O cell-assisted-egglog/matrix.swift -o out/cell-locality-worker/matrix
cargo build --offline --release --example cell_bake_quality --target-dir target/analyzer

# WORKER NEW_OUTPUT_DIR INSTANCES_PER_FAMILY MODE
RAYON_NUM_THREADS=1 target/analyzer/release/examples/cell_bake_quality \
  out/cell-locality-worker/matrix out/bake-quality-profile 128 profile
RAYON_NUM_THREADS=1 target/analyzer/release/examples/cell_bake_quality \
  out/cell-locality-worker/matrix out/bake-quality-check 32 check
RAYON_NUM_THREADS=1 target/analyzer/release/examples/cell_bake_quality \
  out/cell-locality-worker/matrix out/bake-quality-time 512 time

cargo test --offline --test whole_cell --target-dir target/analyzer
```

Use new output directories. Each run writes all five libraries, proof-bearing
`bake.json`, the exact source/input/perturbation `.egg` files and `results.json`.
The same Single repeated eight times must produce one state and unchanged work
counts; this is asserted by the runner. The catalog export regression also
checks native body identity, reader compatibility and refusing overwrite.

A separate control reruns the **previous** five-atom workload with the richest
library. Its 70% Ready fraction does not increase: that workload already had the
needed Single/Get pattern; extra Pair and nested contexts do not restore a
subsumed fact or resolve a branching location.

See [RESULTS.md](RESULTS.md). The structural hypothesis is supported on this
controlled workload. Runtime cost does not improve monotonically: the present
dense row × projection storage/watch reservation becomes expensive for the
richer library. No runtime layout was changed to make the library ablation win.
