# EggFuse · Cell CPU

This branch builds the Rust Cell compiler, offline Bake and standalone CPU executor in one Cargo workspace. The compiler uses the vendored egglog frontend for native analysis. The executor runs checked recipe IR, publishes at the original Run boundaries and exports a logical graph.

```sh
cargo cell-compile --case sum-bits --output out/sum-bits
cargo cell-run --program out/sum-bits/program.json --output out/run
cargo cell-bench --program out/sum-bits/program.json --repeat 5
```

Use fresh output directories. Substitute `raytrace` for the larger Pass 3. Both named cases load a small frozen optimization catalog after compiling the source. The compiler checks the suite checksum, the recipe digest and the executable plans before publishing the artifact. A clean checkout therefore reproduces the 64-block configuration with these three commands.

### Implementation priorities

1. **Preserve source semantics and throughput.** Keep structural-key rewriting, bounded native Bake, marker selection and checked Recipe lowering. Keep ordinary Cell storage, canonical repair, compiled register joins, shared parameter slots, single-write RHS and static constructor aliases. These changes together previously took Raytrace inference from 33.81 to 15.50 s. [Throughput ablation](https://github.com/MilkBlock/eggfuse/blob/508e1c8/tools/cell-compiler/benchmarks/runtime-modules/INFERENCE_THROUGHPUT.md)
2. **Retain the winning matching and handoff routes.** Shared entries, Sieve, column intersections, planned Free Join and range guards handle residual matching. Live Spine receipts, compiled blocks and learned RHS handoffs execute at scheduled entries. Uncovered attribute entries can reuse demanded sibling slots and decoded parents. Runtime cardinality and coverage checks keep their original fallbacks. General connected cuts require offline admission. Fresh untrained sources use eligible narrow contexts. [Spine handoff study](https://github.com/MilkBlock/eggfuse/blob/4da35ee/tools/cell-compiler/benchmarks/runtime-modules/RHS_COMPLETION.md) · [Neighbor ablation](https://github.com/MilkBlock/eggfuse/blob/f985eb7/tools/cell-compiler/benchmarks/runtime-modules/NEIGHBOR_CONTEXT.md)
3. **Remove the remaining experiment machinery from the product path.** The branch drops retained-prefix and refutation matchers, presearch surveys, local saturation, union graft, union impact, local congruence, reference-view absorption, composition takeover and experimental slot transfer. It also removes the older CPU driver, Python/UI experiment suite, historical benchmark patches and duplicated plan exports. Those experiments and their measurements are available on the parent research branch. [Research snapshot](https://github.com/MilkBlock/eggfuse/tree/f985eb7)

The normal commands select a single CPU execution strategy. For checkpoint/resume of research snapshots, historical ablation lanes and debugger sessions, use the parent branch. The registered output includes `program.json`, selected markers, the Bake Cell e-graph, source snapshots and a manifest. The branch keeps the whole vendored egglog dependency and native training code needed by Bake. [Minimal CPU measurements](MINIMAL_CPU.md) · [Run-boundary publication study](docs/CHANGE_BATCH.md) · [Incremental Free Join survey and ablations](docs/SEMIJOIN.md) · [Compiled view consumer blocks](docs/VIEW_CONSUMER_BLOCKS.md) · [Pass 3 family opportunity rankings](docs/family-survey/README.md)

### Custom programs and training

The [two-Run composition experiment](docs/TWO_RUN_BLOCKS.md) transfers a checked
producer-to-consumer projection learned from actual executions. A selected
consumer shares its producer's parameter row at its original Run, with guarded
fallback and explicit lifetime handling. It remains opt-in.

The opt-in [compact block-port backend](docs/PORT_FRAMES.md) keeps stable logical
fact identities while moving completed constructor producers into shared-layout
arrays. It preserves the original Run schedule and provides separate physical
Cell, compact-record and total-producer counts.

An opt-in [deferred intermediate representation experiment](docs/DEFERRED_CELL_FACTS.md)
uses compiler-checked instruction positions and pending unions to postpone
reverse subscriptions and decoded row payloads. Its `off` / `edges` / `on`
Cargo ablation preserves ordinary keys, receipts and Run boundaries.

```sh
cargo cell-compile --source app.egg --output out/app
cargo cell-compile --source app.egg --bake-source training.egg --catalog catalog.json --output out/trained
cargo cell-bench --program out/trained/program.json --reference-program out/app/program.json --repeat 3
```

A reference-program comparison checks the initial graph and every Run's graph, bindings and schedule before timing. It is intended for small regression fixtures. Offline iterative Bake remains available for a seed program containing validated Spine blocks.

```sh
cargo cell-iterate --program out/sum-bits/program.json --output out/learned --rounds 4 --objective rhs-lookups
cargo test --release --locked --workspace --all-features
```

[Compiler and audit commands](tools/cell-compiler/README.md) · [Workload and catalog provenance](workloads/pass3/README.md)
