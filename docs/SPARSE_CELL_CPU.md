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

## Streamed scheduled ports

A compiled Spine block can gather its output slots directly from the published row and any verified dependent port. Scheduled routes now stream in their original witness/atom order; only the general matcher materializes a mutable input environment. Witness vectors for the compiled route are constructed after the port checks succeed. Complete takeovers do not need a residual coverage tree. This keeps the original catalog dispatch, RHS, publication barrier and tie rules.

In three alternating standard-release pairs after warm-up, sparse control / streamed inference medians were 27.025 / 25.163 ms on sum-bits and 9.438 / 8.930 s on Raytrace. CLI medians were 127.78 / 124.27 ms and 11.620 / 11.081 s. Median peak RSS through inference was 157.27 / 157.27 MiB and 2,196.88 / 2,183.80 MiB respectively. Both arms reported 18,735 and 6,915,159 candidate visits. The audited streamed executable matched the frozen ordered RHS, effective writes, Spine trajectory, schedule and final graph. These are incremental standard-release measurements, not a new PGO or 2× acceptance result. [Samples](streamed-port-results.json)

Follow-up paired ablations use the same original artifacts and verification. Reusing a checked projection's scalar input values to compute fingerprints, and using the existing integer-oriented hasher for the projection cache, took streamed search/inference through another comparison. Full inference medians were 25.6 → 24.5 ms and 8.248 → 8.061 s. Median CLI was 121.5 → 120.5 ms and 10.441 → 10.178 s. RSS through inference was within 3 MiB of control on both. The separate experiment retaining output vectors with a shared slot-ID layout was omitted: it changed Raytrace inference from 8.333 to 8.398 s and increased its median RSS by about 2 MiB. Neither experiment changed the visit count or audit digests.

Recording the same Run fields as typed values and using the last changed ordinal for repeat/saturate control reduced sum-bits inference from 24.6 to 23.5 ms and CLI from 120.5 to 119.3 ms. Raytrace inference was 8.188 / 8.251 s and CLI 10.338 / 10.326 s. Median RSS fell by about 3.4 MiB / 36.2 MiB. JSON serialization of the typed log occurs at report generation, outside inference and inside the complete CLI measurement.

For a single unary write on an expression key, the loader can also prove that the proposed i64 comes from an input/literal or one checked addition. The sparse executor reads these ports directly and constructs the producer snapshot only on an effective write. It reserves the same logical ID for every application. The existing scalar merge proof handles min/max/old, including successive improvements to the same key. The final value, actual indexed function and its ordinary publication receipt remain authoritative.

On its own, delaying frame construction was neutral in these samples (24.139 / 24.586 ms and 8.064 / 8.054 s). Enabling the checked merge alongside it changed the paired standard-release medians from 23.904 / 8,121.161 ms to 23.749 / 7,969.169 ms. CLI medians were 119.245 / 10,191.948 ms versus 118.447 / 10,071.665 ms. Through-inference RSS was 153.80 / 2,158.09 MiB versus 153.84 / 2,156.02 MiB. Both audits and the visit counts matched; these samples do not establish a gain from the write projection alone.

### Opposite-workload PGO at fcbb121e

Four measured pairs after one warm-up gave the following medians for the previous tuned sparse build and the new tuned build. RSS is process peak through inference. Both used the frozen 64-block workloads; code generation and training policy matched.

| Pass 3 | Inference, before → after | CLI, before → after | RSS, before → after | Visits | Speedup vs fixed c8ee1a3f |
|---|---:|---:|---:|---:|---:|
| sum-bits | 22.311 → 20.197 ms | 102.917 → 100.122 ms | 157.39 → 153.87 MiB | 18,735 | 1.67× |
| Raytrace | 7.468 → 6.942 s | 9.464 → 8.944 s | 2,199.01 → 2,158.17 MiB | 6,915,159 | 1.95× |

The fixed 2× thresholds are 16.8375 ms and 6.754039 s. Each timing sample validated the schedule and final graph. Ordered RHS, effective writes and Spine were checked with the corresponding separately instrumented build.

### Reverse equality subscriptions

Sparse reverse incidence buckets now reuse the ordered singleton set from the constructor index. The first subscriber is inline, a second distinct witness promotes to a tree, and deletion shrinks a singleton again. Rebuild still visits the displaced representative's subscribers in witness order. In three measured standard-release pairs after warm-up, Raytrace inference changed from 7.954 to 7.943 s, CLI from 10.266 to 10.333 s, and through-inference RSS from 2,158.23 to 2,087.19 MiB. Sum-bits inference was 23.578 / 23.064 ms, CLI 119.959 / 121.011 ms, RSS 153.83 / 153.47 MiB. Visits stayed at 6,915,159 and 18,735. Both full audit runs matched the frozen digests. The reason to keep this change is the Raytrace memory reduction. The earlier separately measured singleton variant in the raw samples used an inline tree header.

Reusing the exact lookup key for both the Spine context cache read and its later fill avoids cloning its value vector. Three paired standard-release medians (control / reuse) were 24.170 / 24.011 ms on sum-bits and 7.982 / 7.857 s on Raytrace. CLI was 125.768 / 126.487 ms and 10.329 / 10.348 s. RSS was 153.45 / 153.45 MiB and 2,090.73 / 2,087.58 MiB. Both full Run audits matched with unchanged visits. This shows no measured CLI gain.

Sharing immutable schedule bodies lets repeat and saturate reuse their decoded children. Source paths and iterations remain occurrence-specific. In three standard-release pairs, sum-bits inference / CLI medians changed from 23.406 / 119.768 to 22.270 / 117.376 ms. Raytrace changed from 7.776 / 10.095 to 7.798 / 10.061 s. RSS was 153.45 / 153.73 MiB and 2,086.98 / 2,089.53 MiB respectively, with unchanged candidates and matching full audit digests.

Two further standard-release experiments kept the ordered audit digests and visits on both workloads. Checking a per-table retirement flag before scanning permanent anchors changed sum-bits inference from 22.981 to 23.182 ms and Raytrace from 7.812 to 7.935 s. CLI was 117.336 / 118.500 ms and 10.138 / 10.230 s, RSS 153.73 / 153.75 and 2,091.17 / 2,098.28 MiB. On top of that variant, borrowing contiguous pivot fields for context-free Spine key reads changed sum-bits inference from 22.129 to 22.112 ms and Raytrace from 7.736 to 7.809 s. CLI was 120.039 / 128.545 ms and 10.062 / 10.182 s, RSS 153.73 / 153.73 and 2,089.70 / 2,099.89 MiB. Neither qualifies for inclusion on speed. Raw samples include the excluded warm-up and three measured pairs.

For a table with only one compiled entry node, the scheduler now decodes its changed witnesses directly into that node. Only tables with multiple nodes use the temporary cross-node row map. The same experiment stopped cloning an entire consumer list on every dispatch and stopped trying to retire old epoch entries immediately after a cold reset had cleared them. In the three-pair ablation over the borrowed-key prototype, sum-bits inference / CLI medians were 22.757 / 118.361 ms before and 22.136 / 118.071 ms after. Raytrace was 7.797 / 10.238 s before and 7.634 / 9.959 s after. Through-inference RSS was 153.73 / 153.72 and 2,089.03 / 2,078.22 MiB. Candidates and all audit hashes matched. These measurements attribute the three scheduler changes together.

Reverse incidence has two ordered consumers: repair inserts into ordered dirty sets, and the direct matcher enumerates witnesses. Sparse subscriptions can therefore use an inline singleton or a hash set, sorting only the filtered candidates actually returned to the matcher. Three paired standard release medians over the register-slice prototype gave sum-bits 22.542 / 21.482 ms inference, 118.141 / 116.467 ms CLI, and 154.17 / 154.14 MiB. Raytrace gave 7.631 / 7.464 s inference, 10.026 / 9.782 s CLI, and 2,088.36 / 2,058.53 MiB. Visits and audited outputs matched.

For scalar writes the compiler-certified permanent-table receipt can accompany the effective RHS directly. Only make an owned key for the fallback that actually queues a key. The three-pair standard release check matched both full audits and candidate counts; sum-bits inference changed from 21.711 to 21.630 ms, CLI 113.615 to 114.433 ms and RSS remained 154.14 MiB. Raytrace inference changed from 7.531 to 7.375 s, CLI 9.821 to 9.761 s and RSS 2,067.08 to 2,064.42 MiB. The separate combined check dropped the two rejected prototypes and compared the entry-cache, register-slice and reverse-set changes together against shared schedule alone: 22.785 / 21.819 ms on sum-bits, 7.689 / 7.364 s on Raytrace.

### Held-out PGO after reverse incidence (0b5c5a17)

Independent clean source snapshot, native CPU, fat LTO, opposite case for training. Four measured pairs after a warm-up. Sum-bits median inference was 18.746 ms, CLI 97.921 ms, RSS through inference 153.72 MiB, 18,735 visits. Raytrace was 6.719 s, 8.689 s, 2,064.41 MiB, 6,915,159 visits. The fixed historical reference gives 1.796× and 2.010×. Raytrace clears its historical cutoff by 35 ms. In the same measurement session the baseline executable was faster than the historical reference: sum-bits 30.425 ms, CLI 126.454 ms, 161.13 MiB, 18,730 visits; Raytrace 12.170 s, CLI 14.901 s, 2,909.26 MiB, 6,916,788 visits. Those paired-arm median ratios are 1.623× and 1.811×. Every timing child matched the frozen schedule and final graph. This run does not establish 2× on both workloads.

Three-pair standard-release ablations on scheduled residual queries and frame packing gave mixed or negative results. The source-ordered residual seed scan with lazily allocated domain vectors changed sum-bits 21.526 → 21.928 ms and Raytrace 7.616 → 7.794 s (CLI 118.909 → 119.235 ms and 9.908 → 10.106 s). The ordered pair collector on top of that scan gave 22.258 → 22.693 ms and 7.559 → 7.506 s (CLI 117.939 → 117.773 ms and 9.896 → 9.807 s). The register range experiment on its own earlier gave 21.983 → 21.618 ms and 7.691 → 7.739 s. The three experiments are excluded from the next clean build. All audited digests and visits matched; raw samples include per-process RSS.
