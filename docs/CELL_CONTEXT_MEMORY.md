# Lifetimes of completed Cell inputs

This experiment starts from `27f098a2` and runs the standalone single-thread Cell CPU engine with the frozen sum-bits and Raytrace Pass 3 artifacts. The existing program, source Run order and 64-unit Bake catalog are held fixed. The execution features introduced here are opt-in.

## Memory attribution

The diagnostic allocator records the requested size and allocation-origin category of each Rust allocation. A shared allocation is counted once regardless of its number of Rc owners. Reallocation retains the original category. Deallocation subtracts the recorded size. This counts live requested bytes, excluding allocator size classes, fragmentation and allocations performed directly by C libraries. Instrumented RSS and timing are excluded from performance comparisons. Allocation provenance can differ from the final owner; for example, a matched parameter vector can move into a persistent Cell.

At the end of baseline Raytrace inference, 1,762.98 MiB of Rust-requested storage remained live. The largest origins were graph storage (494.6 MiB), loading/compiling the artifact (378.5 MiB), matching (215.4 MiB), column indexes (163.4 MiB), shared entry scheduling (149.0 MiB), Spine (121.3 MiB), decoded row payloads (76.9 MiB), and RHS (62.6 MiB). Retained history allocations originated in `History::capture` accounted for 2.3 MiB. Summing independent category peaks would overstate simultaneous memory.

A teardown probe after export independently measured about 323.7 MiB freed by dropping the original Raytrace validation artifact, and 17.1 MiB by dropping its input byte buffer. On sum-bits, dropping that artifact freed about 103.9 MiB. These measurements motivated a separate source-artifact release option. The CLI normally retains this JSON to keep the executable available for inspection and offline planning.

## Consume an entry once all readers have advanced

The compiler already identifies each shared node's rule consumers. A source ruleset occurrence advances every one of its rules, including rules skipped by the ready set. The node can discard a materialized packet only when its epoch is **strictly less** than every consumer's watermark. Equal epochs remain eligible. A rule used by multiple sets advances with the latest actual occurrence.

Consumed packets become empty membership headers. This preserves the exact cardinality used by join planning while releasing the row reference and epoch index. A changed or re-keyed fact can populate the packet again through the existing publication stream. A request for an older watermark marks the node cold and reconstructs its visible rows. The implementation supports both immediate reclamation and a conservative interval of 32 graph epochs. Batching delays reclamation; it never advances a consumer.

Immediate reclamation reduced Raytrace's retained packets from 665,584 to 82,355 and requested live storage from 1,762.98 to 1,686.35 MiB. The corresponding entry-origin allocations fell from 149.0 to 72.4 MiB. There were 882,328 packet retirements across the schedule, so this counter can include several incarnations of one logical witness. On sum-bits, retained packets fell from 3,562 to 2; the 32-epoch variant kept 102.

## Release producer frames of unread completion receipts

The final Raytrace graph contains 1,034,514 logical rows. Generated `CellSpineView*` tables account for 213,334 and `CellSpineApplied*` completion tables for 121,502. The three populated completion tables have no declared query or RHS readers. Their rows still participate in key canonicalization and remain in the final export.

The compiler now derives whether a functional Unit table with an old-value merge has any executable reader. It examines all rule queries, executable RHS table reads and table calls inside merge expressions. Synthetic templates used to materialize initial tables do not constitute readers. Eligibility uses typed structure rather than generated names. The emitted `origin_demand` is validated again against the target program.

An eligible receipt keeps its key, value, row identity, merge and rebuild behavior. Its optional creating-frame reference is omitted. Shared producers remain alive for their other owners. A function table read by an assertion or by a later declared rule keeps its source. The renamed-table regression also rejects a tampered reader certificate.

Raytrace's full live Cells decreased from 601,924 to 485,196. Live embedded writer snapshots increased from 85,989 to 163,541 because removing an unnecessary second owner allows some surviving producers to use the compact representation. Counting both forms, retained producer records decreased from 687,913 to 648,737. All completion-table rows remain available to export. This proof optimizes source-program execution; callers opting into `origin-demand` can observe `None` from the optional producer-origin accessor on unread receipt tables.

## Release validated source IR explicitly

`--release-source-artifact` releases the input buffer and raw validation JSON after all runtime structures have been initialized. The typed plans, structural-key nodes and source SHA remain available to execution. The residual artifact uses a provenance-only schema and is intentionally rejected by `Program::parse`. Offline Bake or revalidation requires reloading the original artifact. Default callers keep the full source. Releasing after execution has started is rejected.

## Uninstrumented measurements

| Raytrace, primary matrix | Inference median | Complete CLI median | Peak RSS |
|---|---:|---:|---:|
| Control | 6.822 s | 13.526 s | 1,864.23 MiB |
| Retire consumed entry packets | 6.813 s | 13.496 s | 1,788.11 MiB |
| Release validation artifact | 6.816 s | 13.627 s | 1,549.02 MiB |
| Both | 6.719 s | 13.538 s | 1,475.88 MiB |

The sum-bits control/retirement medians were 18.181/18.811 ms, with every retirement pair slower. The 32-epoch retirement matrix reduced reclamation frequency; sum-bits medians were 18.701/18.916 ms and Raytrace 6.737/6.712 s. It did not establish a consistent latency gain.

| Final matrix | Control → combined inference | Control → combined complete CLI | Control → combined RSS |
|---|---:|---:|---:|
| sum-bits | 19.158 → 18.740 ms | 116.665 → 116.369 ms | 152.18 → 146.81 MiB |
| Raytrace | 6.746 → 6.717 s | 10.373 → 10.374 s | 1,856.79 → 1,432.68 MiB |

The combined arm includes 32-epoch entry retirement, source-artifact release and origin demand. Raytrace saved **424.11 MiB (22.8%)** peak RSS. Its median paired inference difference was +16.66 ms, with one of four pairs faster. The standalone origin-demand arm was 6.863 s and 1,843.16 MiB. The sum-bits combined median paired difference was −0.362 ms, with six of eight pairs faster. These samples support a memory improvement; they do not establish a substantial inference speedup.

The combined allocator census retained 1,279.69 MiB of requested storage at Raytrace inference completion, down 483.29 MiB. On sum-bits it retained 22.08 MiB, down from 131.99 MiB. Sum-bits RSS stays much higher because the parser/setup high-water has already occurred. In the native runs below, the median RSS was 456.21 MiB. The final Cell control/native memory ratio was about 4.07 and the combined/native ratio about 3.14.

The primary matrix has separate off, entry retirement, artifact release, and combined arms. A second matrix compares periodic retirement. The final matrix compares off, origin-demand alone, and all three changes. Use controls from their own matrix; their absolute wall times differ. There are eight pairs per sum-bits arm and four per Raytrace arm, rotated in fresh processes, without concurrent builds from this task. Complete CLI includes loading, setup, reporting and graph export. RSS is the process high-water mark immediately after inference, before export.

The native comparison reuses the Cargo-built `native-time` runner linked to eggcc's pinned egglog `ebba7bb`. It verifies one worker, runs the original unmarked source, and prints four source-table counts. Two macOS `time -l` measurements gave 462.375 and 450.047 MiB maximum RSS, including parsing, initialization and inference, with no graph export. Cell CPU uses the corresponding frozen marker/Bake-compiled program. These are different representations of the workload; generated Cell tables are included in Cell memory.

## Validation and reproduction

All measured Cell executions match the original final graph and Run schedule. The full audit covers ordered RHS inputs, effective writes, Spine resumptions and trajectory, including the combined changes and forced hash/Sieve collisions on sum-bits. Tests cover the slowest shared reader, equal watermarks, batched release, recovery after an older request, explicit source-artifact release, a late assertion reader, and invalid reader certificates. Allocator tests exercise zeroed aligned allocations and reallocation under a different category.

From this branch, use fresh output directories.

```sh
cargo cell-compile --case sum-bits --output out/sum-bits
cargo cell-compile --case raytrace --output out/raytrace

# Allocation-origin census, separate from speed measurements.
cargo run --locked --release -p eggfuse-cell-runtime \
  --features sparse-cells,view-ports,storage-metrics,allocation-census -- \
  --program out/raytrace/program.json --output out/ray-census

# Completion lifetimes, unread producer release, and source-IR release.
cargo run --locked --release -p eggfuse-cell-runtime \
  --features entry-retirement,origin-demand,view-ports,storage-metrics -- \
  --program out/raytrace/program.json --entry-retire-interval 32 \
  --release-source-artifact --output out/ray-reclaimed
```

Use only `sparse-cells,view-ports,storage-metrics` and omit the two runtime flags for the control. Enable either feature independently for ablation. For ordered audit files add `slot-transfer-audit,spine-shapes`. The allocation ledger is in `report.json`; the separate teardown ledger is `allocation-teardown.json`.

[Measurements, allocation snapshots, native runner hashes and exact input digests](cell-context-memory-results.json)
