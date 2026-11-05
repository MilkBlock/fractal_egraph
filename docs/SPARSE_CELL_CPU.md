# Sparse Cell CPU storage and the next 2× target

The baseline is the standalone single-thread Cell engine at `c8ee1a3f`, with view ports, the frozen 64-unit catalog, and experimental execution routes off. The goal requires at least 2× inference on **each** of sum-bits and Raytrace. The two cases have different size and cost distributions.

## What is known when the constructor is inserted

The previous experiment identified old-Run keepers by following actual same-key repair forwarding. It excluded intermediates returned by a later canonical table lookup in the same Run. All of the resulting candidates occupy Spine anchor operations. The new diagnostic first records their physical source and target IDs. A second run checks the target table, visibility, epoch and final forwarding, then compares the full key at the exact moment of the attempted insertion. IDs in this oracle never route execution.

For the prospective keys, a separate union-find consumes only explicit queued RHS unions in their original order. It is isolated from graph roots, search, insertions, and repair. Every row falls into exactly one category.

| First point where the compared keys agree | sum-bits | Raytrace |
|---|---:|---:|
| Current canonical key at insertion | 0 | 0 |
| After applying only the explicit union prefix already queued at insertion | 34 | 9,508 |
| After also applying later RHS unions in this Run | 2 | 678 |
| Equality still requires information beyond the full explicit RHS queue | 17 | 2,103 |
| Actual older-keeper candidates | 53 | 12,289 |

The prefix shows a future equivalence under pending requests. A consumer can still ask for a pre-repair key, and the anchor still has a precise publication position. The engine keeps ordinary insertion for these nodes. A future execution block that wants to absorb one must supply the old Run's visible key/result through the barrier, preserve union order and offer the right winning receipt after repair. The last row of opportunities is measured relative to an oracle with prior knowledge of the keeper.

## Representation and execution

- A single-member constructor or query bucket stores its witness inline. It grows to an ordered set on a real collision and shrinks back when membership drops. Empty retired hash buckets disappear. Forward subscriptions hold a short sorted list of canonical class IDs. On union only displaced subscribers are marked for repair, and the smaller membership set is moved into the larger one.
- Compact committed rows share one immutable slice. The compiler maps each shared consumer's parameter slots directly onto that row and emits literal/repeated-column checks. Private writes use a separate sparse suffix. A bounded negative directory can establish that a Cell has no displaced equality slots; full-width outliers and positive checks still use the exact parent map.
- A function row can own the immutable slots of its producer. A directory connects the logical Cell ID to that row. Replacing a writer with an explicitly retained historical child promotes the old snapshot before replacing it. The public slot and continuation accessors continue to resolve the original recipe and arguments. **An embedded writer is an actual stored origin** even though it does not occupy a full Cell record.
- A general RHS reserves its usual logical ID and starts in a temporary Cell. A newly owned constructor fact promotes the frame immediately, preserving subsequent RHS lookup semantics. At completion a sole functional owner can embed the frame. Multiple owners or retained children take the full-record route. Dense live records preserve stable logical IDs across swap removal, with a fallback for sparse ID outliers.
- Sorted epoch runs replace per-witness tree nodes on ordered arrivals; retirement uses tombstones and an out-of-order insertion has a tree fallback. Typed unary function keys use a scalar index. Rules whose residual register kernel verifies all full keys can omit a duplicate Free Join entry probe. Expansion domains still use the planned join.
- For compiler-certified tables without visibility-changing operations or hidden initial rows, Spine can coalesce actual receipts in RHS order. It reconciles last-writer ordinals after same-key forwarding. Tables with uncertain visibility keep the ordinary key fallback. A root with a single published anchor can defer its shared handle until routing; multiple readers of that anchor share its single context. Private Spine answers are sealed in stable key order with the previous maximum-depth and equal-depth tie rules.

The branch exposes one experimental execution feature, `sparse-cells`. The earlier ablation commit `1b1ff80a` retains the individual research switches, including optional experiments excluded from this configuration. `storage-metrics` and `keeper-proof` are diagnostics.

## Measured outcome

| Trial (median) | sum-bits inference | Raytrace inference | sum-bits complete CLI | Raytrace complete CLI |
|---|---:|---:|---:|---:|
| Pristine baseline, standard release | 33.675 ms | 13.508 s | 137.13 ms | 16.042 s |
| Sparse, standard release | 27.642 ms | 8.695 s | 129.86 ms | 11.001 s |
| Control, native + held-out PGO | 29.130 ms | 12.049 s | 117.68 ms | 14.303 s |
| Sparse, native + held-out PGO | 23.846 ms | 7.863 s | 111.23 ms | 9.916 s |

The standard release comparison is **1.218× / 1.554×**. The native, profiled sparse build versus the standard baseline is **1.412× / 1.718×**. With the same native/PGO configuration on both sides, the storage/execution change is **1.222× / 1.532×**. Each implementation's profile came from the other workload. Search remains the largest measured inference phase at 10.690 ms and 3.643 s respectively in the profiled sparse build.

| Storage | sum-bits | Raytrace |
|---|---:|---:|
| Full Cell registrations, baseline → sparse | 8,488 → 2,893 | 2,705,661 → 704,559 |
| Full live Cells after inference, baseline → sparse | 2,875 → 2,385 | 687,913 → 601,924 |
| Live embedded writer snapshots at finish | 490 | 85,989 |
| Peak RSS through inference, standard baseline → standard sparse | 161.16 → 157.27 MiB | 2,904.05 → 2,198.34 MiB |
| Peak RSS through inference, native/PGO control → native/PGO sparse | 161.12 → 157.38 MiB | 2,905.98 → 2,194.09 MiB |

The retained full Cells plus embedded snapshots have the same count as the old full Cells in these runs. The savings come from fewer short-lived registrations, smaller representations for surviving origins, and compact indexes. The parent Raytrace run finished with 658,670 singleton constructor hash buckets and 224,477 empty ones; sparse storage retained the 658,670 populated headers. On sum-bits, loading and setup already reached about 146 MiB. These observations support low cardinality in these structures on these programs. They do not estimate the density of arbitrary e-graphs.

## Persistent continuation

Both targets remain open. At this round's baseline medians the thresholds are **16.8375 ms** and **6.754039 s**. Relative to the best measured build the remaining reductions are about **7.01 ms** and **1.109 s**. Re-pair the original control when the machine or build changes.

The next algorithmic experiment should address the producer-to-consumer search and scheduled write together. Sample actual time and occurrence counts per typed join before changing it; compare the work shared across consumers with the residual budget on each input. An anchor-aware provisional row is a narrower memory experiment: 9,508 Raytrace cases have an explicit prefix proof, but this by itself covers only about 1.16% of the 820,439 fact creations. Validate pre-barrier reads, ordered unions and winning receipts before removing any physical insertion. Do not count an oracle or a cached workload result as an executable proof.

Full Cell registration counts, logical Cell IDs, live stored origins, and resident memory measure different things. The allocator includes program metadata, input parsing, row payloads, work queues, and indexes. On macOS, the two RSS samples are process high-water marks taken after setup and immediately after inference, before exporting state. The setup high-water particularly affects the smaller input. Median complete CLI time includes loading, reporting and exporting the same graph.

The original and optimized programs match the frozen final graph and all Run scheduling digests. A separate audited sparse build also matches the ordered RHS inputs, effective writes, resumptions and Spine trajectory. Sum-bits has 1,428 Runs and 3,034 resumptions, maximum chain 12. Raytrace has 16,883 Runs and 663,403 resumptions, maximum chain 29. Sum-bits is also checked with forced fingerprint and Sieve collisions. The ordinary and oracle keeper runs preserve all four audit files and every previous repair/publication barrier.

## Reproduction

Start in a clean checkout and use fresh output directories. The compiler uses the two packaged 64-unit catalogs. The sparse engine checks the same typed artifact.

```sh
cargo cell-compile --case sum-bits --output out/sum-bits
cargo cell-compile --case raytrace --output out/raytrace

cargo run --locked --release -p eggfuse-cell-runtime --features view-ports,storage-metrics,sparse-cells -- \
  --program out/raytrace/program.json --output out/ray-sparse

cargo bench --locked -p eggfuse-cell-compiler --bench cell --features sparse-cells,view-ports -- \
  --program out/raytrace/program.json --repeat 6
```

Omit `sparse-cells` for an off control on this branch. For the exact pre-research timing baseline use `c8ee1a3f` and enable `view-ports`; the reported paired baseline only added the two external resource samples. Each measured CLI process is fresh. The Cargo bench harness is useful for checking inference separately.

```sh
flags=keeper-proof,view-ports,slot-transfer-audit,spine-shapes
cargo run --locked --release -p eggfuse-cell-runtime --features "$flags" -- \
  --program out/raytrace/program.json --rhs-local-width 8 --output out/record-ray
cargo run --locked --release -p eggfuse-cell-runtime --features "$flags" -- \
  --program out/raytrace/program.json --rhs-local-width 8 \
  --keeper-pairs out/record-ray/keeper-proof.json --output out/replay-ray
```

The native-build comparison is separate. For **both** control and sparse builds use `CARGO_PROFILE_RELEASE_LTO=fat`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1`, and `RUSTFLAGS="-C target-cpu=native -C profile-generate=ABSOLUTE_DIRECTORY"`. Run the instrumented executable on the other workload with `LLVM_PROFILE_FILE=ABSOLUTE_DIRECTORY/%m.profraw`, merge with `llvm-profdata merge`, and rebuild with `-C profile-use=ABSOLUTE_PROFILE_FILE` in place of `-C profile-generate`. Each implementation gets its own held-out profile. Measure the resulting binary without training or audit instrumentation.

[Raw timing samples, memory, hashes, phase measurements and per-rule proof counts](sparse-cell-results.json)
