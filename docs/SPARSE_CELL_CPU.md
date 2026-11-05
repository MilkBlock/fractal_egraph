# Sparse Cell CPU storage and the next 2× target

The baseline is the standalone single-thread Cell engine at `c8ee1a3f`, with view ports, the frozen 64-unit catalog, and experimental execution routes off. The goal requires at least 2× inference on **each** of sum-bits and Raytrace. The two cases have different size and cost distributions.

## Correction to the interim paired controls

The archived standard reference for commit c8ee1a3f is the binary identified in `sparse-cell-results.json` by SHA-256 `f9eea9786662d6993d3fcd5387f4e99ec0b5b088f788329d84284f1262374466` (`original-bin`). The original 33.675 ms and 13,508.078 ms measurements, and their fixed cutoffs, use that reference. A subsequent script instead used a development executable named `base` (SHA-256 `c23d04db683671a02baaa26ca7af99c1aacecfab2a7aa11b37847dd875a086b9`). It does not match the archived reference hash. The interim paired comparisons of that executable below do not establish a same-session speedup against the archived c8ee1a3f executable. Candidate timings, frozen digests, within-experiment ablations and fixed-cutoff comparisons are unaffected. A paired recheck uses the verified archived binary directly.

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

Independent clean source snapshot, native CPU, fat LTO, opposite case for training. Four measured pairs after a warm-up. Sum-bits median inference was 18.746 ms, CLI 97.921 ms, RSS through inference 153.72 MiB, 18,735 visits. Raytrace was 6.719 s, 8.689 s, 2,064.41 MiB, 6,915,159 visits. The fixed historical reference gives 1.796× and 2.010×. Raytrace clears its historical cutoff by 35 ms. In the same measurement session the development control executable was faster than the historical reference: sum-bits 30.425 ms, CLI 126.454 ms, 161.13 MiB, 18,730 visits; Raytrace 12.170 s, CLI 14.901 s, 2,909.26 MiB, 6,916,788 visits. Those development-control median ratios are 1.623× and 1.811×. Every timing child matched the frozen schedule and final graph. This run does not establish 2× on both workloads.

Three-pair standard-release ablations on scheduled residual queries and frame packing gave mixed or negative results. The source-ordered residual seed scan with lazily allocated domain vectors changed sum-bits 21.526 → 21.928 ms and Raytrace 7.616 → 7.794 s (CLI 118.909 → 119.235 ms and 9.908 → 10.106 s). The ordered pair collector on top of that scan gave 22.258 → 22.693 ms and 7.559 → 7.506 s (CLI 117.939 → 117.773 ms and 9.896 → 9.807 s). The register range experiment on its own earlier gave 21.983 → 21.618 ms and 7.691 → 7.739 s. The three experiments are excluded from the next clean build. All audited digests and visits matched; raw samples include per-process RSS.

Fresh reverse subscriptions now install their class IDs through one forward-map entry; every subscription site supplies the table ID it already resolved. Existing subscriptions still reconcile old canonical representatives before removing edges. Sum-bits standard-release inference / CLI medians changed from 22.491 / 121.444 to 20.886 / 116.062 ms in three measured pairs. Raytrace was 7.378 / 9.655 versus 7.371 / 9.766 s. RSS was 154.17 / 154.19 MiB and 2,073.75 / 2,066.61 MiB. Both full digest audits passed and visits remained 18,735 / 6,915,159. These incremental runs used the experimental typed-control stack.

Sealed complete Spine bindings can own a move-only token into a per-Run cursor vector. Consuming the token moves the original winner to the RHS; the publication boundary releases the vector's slots. In the measured stack, 3,034 and 663,403 resumptions and the full winning prefix-tree digests matched. Sum-bits standard-release inference / CLI medians were 21.944 / 117.892 ms before and 21.004 / 116.500 ms after. Raytrace was 7.418 / 9.739 s before and 7.438 / 9.716 s after. RSS was 154.13 / 154.11 MiB and 2,057.17 / 2,064.95 MiB. A preceding experiment that interned diagnostic counters helped Raytrace but regressed sum-bits (21.295 → 22.905 ms); its implementation is excluded from the clean candidate.

Read the producer frame once per RHS argument list, use inline storage for short ephemeral key/primitive arguments, and send already-canonical arguments to the keyed virtual set probe. In three standard release pairs, sum-bits inference was 23.424 / 22.160 ms and Raytrace 7.457 / 7.345 s. CLI was 122.789 / 123.473 ms and 9.803 / 9.702 s. RSS was 154.28 / 154.77 MiB and 2,085.28 / 2,098.30 MiB. Both workloads preserved candidate counts and audited digests. The separate experiment duplicating executable templates with resolved Copy operands showed no paired speed gain and added memory; its control and samples are recorded separately.

Sparse Sieve guards with one or two projected fingerprint columns now check the stored support set directly. Wider projections keep Bloom membership and the exact matcher still checks source values. Compared with the same warm-index setup, sum-bits candidates were 18,735 / 18,732, inference 21.525 / 20.673 ms, CLI 117.624 / 119.120 ms and RSS 154.34 / 154.33 MiB. Raytrace was 6,915,159 / 6,879,302 visits, 7.344 / 7.355 s inference, 9.692 / 9.653 s CLI and 2,078.88 / 2,080.78 MiB. Three measured pairs after warm-up. Both Run audits, including the winning Spine trajectory, matched the frozen digests. The separate warm-index feed trial is recorded as its own arm.

A repeated-execution sampling profile of sum-bits found 2,239 samples under execute_until, including allocation and destruction of recursively cloned control JSON. Source schedule instructions are now decoded at setup into a typed tree. Child values move into their executable node once; serialization reconstructs checkpoint JSON including metadata and missing repeat bodies. The new round-trip test passed. With typed control and owned Run path vectors the earlier ablation gave 21.618 / 21.228 ms on sum-bits and 7.402 / 7.416 s on Raytrace. Moving child nodes without subtree clones subsequently changed inference 21.180 / 20.268 ms and 7.417 / 7.317 s; CLI 116.649 / 115.138 ms and 9.670 / 9.547 s; RSS 154.31 / 153.56 MiB and 2,082.61 / 2,075.95 MiB. The setup work is included in CLI. Both audits matched.

### Held-out PGO at d7829e3b

Four alternating measured pairs after one excluded warm-up, using a clean source archive of the local commit. The same source passed 21 all-feature runtime tests and both full audits. Forced zero-bit column fingerprints and forced Bloom collisions together preserved the full sum-bits audit.

| Case | Inference, baseline / candidate | Full CLI, baseline / candidate | Peak RSS through inference, baseline / candidate | Visits, baseline / candidate | Fixed speedup | Development-control ratio |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 32.057 / 18.553 ms | 131.763 / 99.095 ms | 161.14 / 153.02 MiB | 18,730 / 18,732 | 1.815× | 1.728× |
| Raytrace | 12.146 / 6.626 s | 14.861 / 8.754 s | 2,910.78 / 2,064.81 MiB | 6,916,788 / 6,879,302 | 2.039× | 1.833× |

The paired arms ran sequentially on the same machine. All four measured Raytrace candidate times (6.603–6.736 s) cleared the fixed 6.754039 s cutoff. Sum-bits was 1.715 ms above 16.8375 ms. No simultaneous 2× result.

Clean semi-naive rule occurrences now advance their watermarks directly when per-rule timing and survey instrumentation are off. In the standard-release stack, paired inference medians changed from 22.822 to 20.534 ms for sum-bits and 7.431 to 7.351 s for Raytrace. The sum-bits control varied across measurement groups; this comparison alone does not establish a 2.288 ms gain on the clean PGO build. In a separate ablation cold Sieve summaries consumed the column's fingerprints directly, avoiding a sorted witness copy and a second lookup per witness. Inference was 20.367 / 20.127 ms and 7.419 / 7.324 s. CLI was 116.726 / 115.241 ms and 9.841 / 9.857 s. Both changes matched frozen audits and left candidate visits unchanged. Maintaining per-rule counts of missing tables between Runs slowed Raytrace in its own paired experiment (7.344 / 7.529 s), so that experiment is excluded from the clean configuration.

Sparse table directories now reuse the ordered-run representation used by epoch frontiers. Monotone physical witnesses append in order, retired slots are masked and compacted, and an out-of-order new key promotes the directory to an ordered tree. Three paired medians in the experimental stack were 19.922 / 19.519 ms and 7.324 / 7.223 s inference, 114.094 / 114.766 ms and 9.654 / 9.576 s CLI, and 154.05 / 153.97 MiB and 2,075.11 / 2,058.22 MiB. Both full audits matched. A separate mixed-update test checks iteration, duplicate insertion and retirement against a BTreeSet.

The preceding residual-domain refinement narrowed candidates from 18,732 to 18,645 and 6,879,302 to 6,737,948 after additional keys became available. Its paired times were 20.226 / 20.505 ms and 7.315 / 7.321 s. Index work remains inside inference. Refining stored domains and projecting dedup keys are still experimental; these candidate counts are not attributed to the table-directory change.

Seal a Run as a sequence of rule-owned binding vectors. The original per-rule vectors now transfer to their commit positions directly. Flattening is only requested by diagnostics that need it. On the ordered-table prototype, inference medians for three pairs were 20.234 / 19.752 ms on sum-bits and 7.236 / 7.212 s on Raytrace. CLI was 117.753 / 117.259 ms and 9.544 / 9.563 s and RSS 153.98 / 153.94 MiB and 2,061.05 / 2,043.73 MiB. The per-Run RHS and effective write order, Spine audit, schedule and final graph matched.

### Held-out PGO at 76d688f0

Clean source archive, 22 all-feature runtime tests, both full audits, one warm-up then seven measured pairs for sum-bits and five for Raytrace. A CPU-bound exploratory analysis overlapped the first Raytrace series; the table uses a fresh complete sequential repeat after stopping that analysis. The excluded series is retained in the raw results.

| Workload | Inference, development control / candidate | CLI, development control / candidate | RSS, development control / candidate | Visits, development control / candidate | Fixed speedup | Development-control ratio |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 30.731 / 18.343 ms | 128.684 / 100.211 ms | 161.14 / 153.00 MiB | 18,730 / 18,732 | 1.836× | 1.675× |
| Raytrace | 12.450 / 6.706 s | 15.178 / 8.721 s | 2,909.14 / 2,054.61 MiB | 6,916,788 / 6,879,302 | 2.014× | 1.856× |

The measured candidate ranges were 17.782–18.972 ms and 6.642–6.737 s. Sum-bits needs 1.5055 ms less than this median to reach its fixed cutoff. These results predate the later rule-owned binding-vector change.

For a compiler-planned unary Free Join projection, the shared column bucket is already its full witness domain. Admission can reuse the bucket's maintained view directly across consumers, avoiding the per-projection copy of the versioned membership cache. Multi-column projections retain ordinary admission. On the experimental stack this changed sum-bits inference from 20.855 to 19.769 ms, CLI from 119.886 to 121.084 ms, and RSS from 153.95 to 153.56 MiB. Raytrace inference fell from 7.395 to 7.207 s, CLI from 9.708 to 9.519 s and RSS from 2,039.78 to 1,997.38 MiB. Candidate visits and both full audits matched.

The compiler also recognizes an expansion immediately followed by a checked i64 sum of two row columns whose result is already bound. On a second request for the identical shared witness domain in the same table version, the executor partitions that domain by checked sum and keeps its original witness order. The original comparison still runs. Only one domain per site is retained, and hidden rows and overflowing sums cannot supply matches. In three standard-release pairs, sum-bits inference fell from 19.552 to 18.738 ms (CLI 116.310 / 116.703 ms, RSS 153.45 / 153.83 MiB) and Raytrace inference changed from 7.158 to 7.230 s (CLI 9.629 / 9.525 s, RSS 1,989.66 / 1,998.33 MiB). Matching visits dropped from 18,645 to 18,619 and from 6,737,948 to 6,657,105 on that stack. Partition-building reads are counted separately and included in timing. Both full audits matched.

### Held-out PGO at dcb66daa

Clean source archive with shared unary column views, rule-owned binding vectors and the checked-sum grouping. All 22 all-feature runtime tests passed. Independent audits matched both cases' ordered RHS, effective writes, winning Spine, schedule and final state. All timing processes checked schedule and final graph.

| Workload | Inference, development control / candidate | Full CLI, development control / candidate | Peak RSS, development control / candidate | Visits, development control / candidate | Fixed speedup | Development-control ratio |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 30.472 / 17.544 ms | 128.738 / 99.130 ms | 161.13 / 152.89 MiB | 18,730 / 18,706 | 1.919× | 1.737× |
| Raytrace | 12.191 / 6.431 s | 15.068 / 8.412 s | 2,907.56 / 1,984.28 MiB | 6,916,788 / 6,798,459 | 2.101× | 1.896× |

Sum-bits needs a further 0.7065 ms reduction for the fixed 16.8375 ms target; its seven measured candidate processes ranged from 17.026 to 18.339 ms. All five Raytrace candidate processes were below 6.754039 s (6.253–6.570 s). Separate stack diagnostics show the cost behind computed-domain pruning: Raytrace scanned 20,416 rows to build arithmetic groups and skipped 80,843 matching visits. Sum-bits scanned 29 to skip 26. These counts are a physical work measure, distinct from the end-to-end timings.

For complete unary equality-key reads used at several compiled kernel sites, matching can use a 32-slot table-local receipt cache. It checks the exact input and Run epoch, caches both absence and a full row including its ownership epoch, and performs the ordinary read on a miss. It is only used by scheduled reads in the immutable search phase. In three standard-release pairs over the late-intersection experiment sum-bits was 19.657 / 18.980 ms, CLI 118.358 / 117.192 ms, RSS 153.98 / 154.05 MiB. Raytrace median inference was 7.023 / 6.944 s but two of the three individual pairs were slower, and CLI medians were 9.312 / 9.396 s. The Raytrace speed difference is inconclusive. Visits and winning audit digests matched on both cases.

A clean three-arm trial at 22f3f517 checked whether the same unary receipt should survive Runs until the target table version changes. Epoch versus table-version cache medians were 19.723 / 20.015 ms for sum-bits and 7.203 / 7.230 s for Raytrace. Full CLI was 113.283 / 115.591 ms and 9.586 / 9.515 s. The third arm also tracked queued repairs through function flags and the per-table dirty-fact set; its medians were 19.866 ms and 7.097 s. All three arms passed both full audits. The default cache keeps the Run boundary. Earlier physical-storage trials for inline fact headers and a separate table-cardinality vector are recorded but not included: the former raised Raytrace median RSS by 37.69 MiB and the latter made all three Raytrace inference pairs slower.

Each compiled table now owns one witness directory. Its table kind supplies the tag; a constructor stores the unsigned (Cell, operation) pair as an order-preserving 64-bit word and a function stores its original row index. This also provides cardinality without a second maintained counter. In the independent clean 22f3f517 ablation, sum-bits inference changed from 19.471 to 19.367 ms, CLI 116.759 to 119.343 ms and RSS 152.84 to 152.83 MiB. Raytrace changed from 7.197 to 7.128 s, CLI 9.664 to 9.502 s and RSS 2,000.69 to 1,997.61 MiB. Both full audits and visit counts matched.

Building a cold Sieve summary now inserts its certified support first and populates the final Bloom bitset once. All child summaries have already been constructed, and no parent probes the new summary before it is complete. Incremental publication retains its prior per-key propagation. On the clean Run-port control, three paired medians were 19.737 / 19.472 ms and 7.119 / 7.080 s inference, 117.826 / 119.027 ms and 9.423 / 9.419 s CLI, and 152.83 / 152.81 MiB and 2,006.81 / 2,005.09 MiB RSS. Visits and the two full Run audits matched.

### Held-out PGO at 98792cd9

Both complete Run audits passed at the clean commit, as did the 22 all-feature runtime tests. With native CPU, fat LTO, one codegen unit and opposite-workload training, nine sum-bits / five Raytrace pairs gave the following medians after one warm-up.

| Case | Inference, development control / candidate | CLI, development control / candidate | RSS, development control / candidate | Visits, development control / candidate | Fixed speedup | Development-control ratio |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 30.813 / 17.304 ms | 130.916 / 98.576 ms | 161.14 / 152.97 MiB | 18,730 / 18,706 | 1.946× | 1.781× |
| Raytrace | 12.436 / 6.517 s | 15.180 / 8.497 s | 2,906.50 / 1,987.16 MiB | 6,916,788 / 6,798,459 | 2.073× | 1.908× |

No measured sum-bits process cleared the fixed 16.8375 ms threshold (16.996–19.608 ms). All measured Raytrace processes cleared 6.754039 s (6.389–6.610 s). The remaining sum-bits reduction is 0.4665 ms.

A shared slot in two table atoms requires their projected columns to overlap. The checked Free Join plan now interns those pairs. Matching can reject the whole rule when its maintained indexes are current, one side has at most eight distinct fingerprints, and the two sets are disjoint. Both table versions guard the certificate. A hash collision can hide a rejection but cannot produce a false absence. Standard-release medians over three pairs were 20.557 / 20.196 ms and 7.276 / 7.245 s inference, 118.274 / 116.742 ms and 9.592 / 9.567 s CLI. RSS was 152.86 / 153.36 MiB and 1,991.69 / 1,992.53 MiB. Visits were 18,706 / 18,693 and 6,798,459 / 6,798,165. Both full audits matched.

In a checked mixed register kernel, only the expansion atom consumes a witness list. A single-column guard for a later complete-key read now asks the existing column index for empty, singleton or multiple support. The singleton checks its exact delta owner. A positive guard then hands control to the later read without materializing an unused subtrie. Kernel absence, conflicting range plans and instrumented projection paths retain their existing handling. On clean 545d3022 all three sum-bits inference pairs improved: median 20.530 / 19.687 ms, CLI 117.302 / 116.519 ms, RSS 153.36 / 152.95 MiB. Raytrace was 7.240 / 7.214 s, CLI 9.609 / 9.587 s, RSS 2,000.83 / 1,990.45 MiB. Both full audits matched, with 18,693 / 6,798,165 visits in both arms.

Partial executor-owned Spine answers now carry the same move-only origin as complete takeovers. The matcher inserts them before residual matches and its stable binding-key dedup retains the Spine answer on ties. The originating cursor therefore arrives at the original RHS ordinal without a second key copy or origin-table probe. The public standalone program retains its keyed route. Full audits matched on sum-bits and Raytrace. In three clean-snapshot standard-release pairs, inference was 20.162 / 19.749 ms and 7.250 / 7.179 s, CLI 115.824 / 114.639 ms and 9.689 / 9.644 s, RSS 153.36 / 152.91 MiB and 1,993.47 / 1,988.64 MiB. Candidate visits matched. The separate bounded two-expansion register-plan trial increased visits on both cases (18,706 → 18,752 and 6,798,459 → 6,801,515) and provided no median inference gain; it is excluded.

A zero-argument table has only one possible key. When its typed directory is empty or has a single witness, a keyed read can report absence or fetch that exact member without hashing the empty key. Row visibility, epoch and receipt still come from the original physical record. In a separate clean three-pair ablation this changed sum-bits inference from 20.209 to 19.821 ms, CLI 118.508 to 120.707 ms and RSS 152.94 to 152.98 MiB. Raytrace changed from 7.135 to 7.029 s, CLI 9.486 to 9.405 s and RSS 1,997.78 to 1,992.11 MiB. Candidate counts and full audits matched.

### Held-out PGO at dbf430f5

The two new join guards retained the full frozen audits, including sum-bits under forced collisions. The all-feature runtime suite passed 22 tests. On 545d3022 the compiler integration suite passed 15 tests with sparse Cells, view ports and iterative Bake.

| Case | Inference, development control / candidate | CLI, development control / candidate | RSS, development control / candidate | Visits, development control / candidate | Fixed speedup | Development-control ratio |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 31.126 / 17.735 ms | 127.771 / 100.406 ms | 161.17 / 153.09 MiB | 18,730 / 18,693 | 1.899× | 1.755× |
| Raytrace | 12.423 / 6.433 s | 15.214 / 8.440 s | 2,908.23 / 1,988.91 MiB | 6,916,788 / 6,798,165 | 2.100× | 1.931× |

A second, directly interleaved comparison of the existing 98792cd9 and dbf430f5 optimized executables gave identical sum-bits medians of 17.6625 ms across ten measured pairs. Raytrace was 6.452 / 6.443 s across three pairs. Fewer witnesses and a faster standard-release arm did not establish a tuned inference win here. The new plan performed membership-only probes 1,652 / 206,645 times and rejected whole rules 239 / 397 times. This revision did not satisfy both fixed thresholds.

Fact and function reads now return already-decoded rows through a small inlinable path. The separate fallback reconstructs the fields from the original Cell or held function key. Constructor reconstruction reserves the result column together with its operands. On the experimental stack, inference medians across three pairs changed from 19.157 to 18.751 ms and 6.923 to 6.867 s. CLI medians increased from 113.652 to 114.658 ms and 9.205 to 9.335 s, RSS was 153.92 / 153.95 MiB and 2,035.25 / 2,031.25 MiB. Both full audits and visits matched. This change is being evaluated for inference speed.

### Held-out PGO at 209583e7

This clean snapshot includes sealed partial Spine winners and nullary directory reads. All 22 runtime tests and both full audits passed. Nine sum-bits and five Raytrace pairs after warm-up gave the following medians.

| Case | Inference, development control / candidate | CLI, development control / candidate | RSS, development control / candidate | Visits, development control / candidate | Fixed speedup | Development-control ratio |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 30.465 / 17.442 ms | 129.373 / 98.874 ms | 161.14 / 153.06 MiB | 18,730 / 18,693 | 1.931× | 1.747× |
| Raytrace | 12.136 / 6.267 s | 14.925 / 8.238 s | 2,914.38 / 1,994.94 MiB | 6,916,788 / 6,798,165 | 2.155× | 1.936× |

The sum-bits median is 0.6045 ms above the fixed cutoff. Every Raytrace candidate process passed its fixed cutoff. The high-level two-workload objective remains open.

The generic matcher can decide two priority classes directly from bound slots: executable primitives first, then complete table keys, in source atom order. It counts partial index domains only when neither cheaper class is available; the first zero-size partial domain is minimal. A complete key also returns the sealed row directly to the ordinary Sieve, ownership and binding checks, removing the witness-vector conversion and second row fetch. On a clean source comparison, sum-bits median inference / CLI were 20.418 / 116.907 ms for the original, 20.114 / 115.026 ms for classification alone and 20.043 / 116.900 ms for both. Raytrace was 7.184 / 10.296, 7.164 / 10.213 and 7.161 / 10.182 s. RSS was 153.41 / 152.94 / 152.95 MiB and 1,989.03 / 1,998.17 / 1,994.69 MiB. All three full audits and visit counts matched. This ablation measures the standalone Cell solver.

A consumer's zero- or one-column Spine port key now stays inline during lookup and context refresh. Wider keys spill to owned storage and continue through the same exact lookup, visibility and ownership checks. Three clean pairs gave 19.872 / 19.512 ms on sum-bits and 7.178 / 7.178 s on Raytrace. CLI medians were 114.079 / 113.359 ms and 10.287 / 10.282 s; RSS 153.41 / 153.39 MiB and 1,991.83 / 1,988.25 MiB. The frozen audits and visits matched. The separate generic planner census confirmed 2,659 → 2,494 indexed cost estimates on sum-bits and 427,155 → 403,863 on Raytrace. Direct consumption of keyed rows saved a further 52 / 4,162 actual witness-to-row reads.

The loader can classify a marker-producing RHS once. Spine's per-ruleset counters can also keep the most recent set in a single slot; the current count is included even when reported mid-Run. In a clean three-way ablation, sum-bits inference was 20.181 ms (control), 20.237 ms (classification alone), and 19.684 ms (classification plus recent-set counting). Raytrace was 7.244 / 7.191 / 7.180 s; the combined arm beat control in all three pairs on both workloads. Combined CLI medians were 118.913 ms / 9.577 s and RSS was 153.41 / 2,001.34 MiB. The measured prototype copied the name on a set transition; the retained code instead moves its existing string between the settled map and the current slot. Source-semantic outputs matched in both audits.

Adjacent Run phases now share their boundary timestamp. Full phase fields are retained; optional binding-audit and replay work keep a separate boundary. This removes redundant clock calls during inference. Three matched pairs on the clean control gave 20.015 / 19.817 ms and 7.210 / 7.170 s inference, 117.838 / 113.525 ms and 10.149 / 10.221 s CLI. RSS was 152.94 / 152.95 MiB and 1,997.53 / 1,989.14 MiB. The audited trace and candidate counts matched. Separately sorting routed cursors only after successful guards regressed sum-bits, and flattening all subtries into one Run vector regressed its inference from 19.549 to 21.065 ms; neither was retained.

Free Join seed admission can keep its first retained subtrie inline until the consumer uses it; larger lists spill, preserving the same witness order and leases. This targets the 12,880 sum-bits and 3,653,552 Raytrace allocation/reallocation calls recorded inside entry admission by the separate allocation census at 18272c10. In three paired standard-release processes per workload after warm-up, sum-bits inference was 19.610 / 19.076 ms and Raytrace 6.946 / 6.879 s. CLI was 113.847 / 112.613 ms and 9.730 / 9.747 s; RSS was 152.91 / 152.86 MiB and 1,988.97 / 1,986.52 MiB. Both full audits and visits matched.

### Held-out PGO at b113bce0

The clean snapshot passed all 23 all-feature runtime tests and both workload audits. The frozen artifact hashes matched; each contains 32 seed blocks and 32 learned joins. After one warm-up, nine sum-bits and five Raytrace alternating measured pairs gave:

| Case | Inference, development control / candidate | CLI, development control / candidate | Peak RSS, development control / candidate | Visits, development control / candidate | Fixed speedup | Development-control ratio |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 30.833 / 17.218 ms | 129.397 / 97.607 ms | 161.17 / 152.94 MiB | 18,730 / 18,693 | 1.956× | 1.791× |
| Raytrace | 12.220 / 6.210 s | 16.352 / 9.595 s | 2,908.03 / 1,984.52 MiB | 6,916,788 / 6,798,165 | 2.175× | 1.968× |

All measured Raytrace candidate times were 6.202–6.257 s; sum-bits was 17.058–18.039 ms. Its median requires another 0.3805 ms reduction to reach 16.8375 ms. CLI includes loading, reporting and exporting, including the slower complete CLI for both Raytrace arms in this session.

### Recheck with the SHA-verified original binary, b113bce0

The control exactly matched the archived SHA-256 for c8ee1a3f. Comparing its archived runtime source against that commit showed just the CLI RSS/report sampling and dependency declaration, with the inference elapsed time sampled before peak RSS. Both optimized arms used the same source and frozen catalogs. The enriched arm increased LLVM's capacity for value-profile counters during the opposite-workload training build. The training logs went from 20 counter-capacity warnings to zero. The timed executable still runs one CPU thread and does not collect profiles.

| Case and arm | Inference median | CLI median | Peak RSS | Visits | Fixed speedup | Paired verified-original ratio |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits original | 30.715 ms | 127.133 ms | 161.16 MiB | 18,730 | — | — |
| sum-bits regular PGO | 17.637 ms | 98.464 ms | 152.94 MiB | 18,693 | 1.909× | 1.742× |
| sum-bits enriched PGO | 17.086 ms | 97.020 ms | 152.94 MiB | 18,693 | 1.971× | 1.798× |
| Raytrace original | 11.998 s | 14.742 s | 2,907.02 MiB | 6,916,788 | — | — |
| Raytrace regular PGO | 6.046 s | 8.026 s | 1,990.89 MiB | 6,798,165 | 2.234× | 1.985× |
| Raytrace enriched PGO | 6.222 s | 8.184 s | 1,989.55 MiB | 6,798,165 | 2.171× | 1.928× |

Each arm had one excluded warm-up and nine / five measured processes. Every time sample matched the frozen schedule and final graph; the source's independent full audits are reported above. The enriched sum-bits candidate range was 16.853–18.385 ms. Its median still needs a 0.2485 ms reduction for the fixed cutoff. The same-case paired control remains a separate test of a twofold improvement on the present machine.

The frozen schedule reaches the same 208 distinct source paths in 1,428 sum-bits Runs and 16,883 Raytrace Runs. Source tree nodes can own shared path slices; loop tasks and the Run ledger borrow that immutable storage, while iteration occurrences retain their own sequence. Checkpoint JSON preserves the original arrays, including restored tasks whose parent path must be reconstructed; Rust task and ledger path/iteration fields now expose Rc slices. Standard-release inference medians were 19.478 / 19.046 ms and 6.953 / 6.874 s, CLI 112.888 / 112.953 ms and 9.487 / 9.461 s, RSS 152.91 / 152.31 MiB and 1,990.84 / 1,981.70 MiB. Both full audits and the checkpoint wire round-trip passed. The retained implementation also checks that a task uses its compiled parent path before reusing a child's cached path.

A compiled ruleset now has a bit per source position. A publication that transitions a reader from clean to dirty wakes its positions across all sets. Matching visits the set bits in original order. A clean rule's logical watermark advances at each of its sets' completed source occurrences, even when it does not need a matcher dispatch. Survey and per-rule profiling use their original observation loop. The same rule may occur in several sets or more than once in one set. Three paired inference medians were 19.795 / 18.886 ms and 6.983 / 7.002 s, CLI 114.954 / 113.296 ms and 9.587 / 9.643 s, and RSS 152.89 / 153.36 MiB and 1,983.75 / 1,993.19 MiB. Each sum-bits inference pair improved (0.900–1.619 ms); Raytrace's difference was mixed. Candidates and all Run audit hashes matched.

An empty compiled ready set can seal an idle source Run immediately only after checking the graph, entry scheduler, column notifications and Spine's pending publications, origins and leases. It still advances the epoch and ruleset watermark, appends the real Run, and emits all empty RHS/write audit boundaries. Empty graph changes alone do not qualify. Three paired inference medians were 19.610 / 19.129 ms and 7.020 / 7.074 s; CLI was 114.740 / 112.592 ms and 9.577 / 9.607 s, RSS 153.38 / 153.36 MiB and 1,991.56 / 1,995.59 MiB. Visits and both full audits matched. This arm is under clean held-out verification for the sum-bits cutoff.

The shared entry scheduler now appends sparse change headers and coalesces them in witness order at refresh. A dormant node periodically compacts once its pending list exceeds twice the last distinct size or 64 entries, bounding repeated notifications. Run reports include both consumed and still-pending distinct notifications. On three clean pairs, sum-bits inference fell from 19.492 to 19.005 ms, CLI 115.625 to 114.281 ms, RSS 152.55 / 152.61 MiB. Raytrace inference fell from 7.020 to 6.956 s, CLI 10.372 to 10.286 s, RSS 1,986.72 / 1,980.86 MiB. Matching visits and both full audits matched.

### Verified-original pairs at 29d3c5e0

A clean native build with enlarged offline profile counters was trained on the opposite workload for each test. The pinned original was verified by SHA before running eleven measured sum-bits pairs and seven Raytrace pairs after warm-up. The 24 all-feature runtime tests and both full audits passed. On the ordinary configuration a missing hasher type annotation prevented compilation; e5a74879 fixed it. The repaired snapshot passed 12 ordinary runtime tests, nine no-default-feature runtime tests and 15 sparse/view-port/iterative compiler integration tests.

| Case | Inference, archived original / candidate | CLI, original / candidate | RSS, original / candidate | Visits, original / candidate | Fixed speedup | Paired archived-original ratio |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 30.777 / 17.117 ms | 127.997 / 98.876 ms | 161.16 / 152.77 MiB | 18,730 / 18,693 | 1.967× | 1.798× |
| Raytrace | 12.334 / 6.292 s | 15.454 / 8.664 s | 2,907.16 / 1,987.91 MiB | 6,916,788 / 6,798,165 | 2.147× | 1.960× |

The quiescence proof covered 769 / 1,428 and 7,551 / 16,883 source Runs. Each still appeared in the schedule and per-Run audits. Forced collisions on sum-bits also matched all frozen digests. The sum-bits median is 0.2795 ms over the fixed 16.8375 ms cutoff; an individual 16.777 ms process does not satisfy median acceptance.
