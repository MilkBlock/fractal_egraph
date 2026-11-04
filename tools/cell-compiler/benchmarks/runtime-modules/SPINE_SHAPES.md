# Spine combinations and profile-guided consumer blocks

This study uses the two frozen eggcc Pass 3 artifacts on the standalone Rust Cell CPU. A transition is counted only when an applied RHS takes the winning Spine origin. Multiple published anchors, eligible seeds, unsuccessful port checks and deduplication losers do not count as resumptions. Unresumed roots are outside the distribution.

## Which combinations occur?

| Quantity | sum-bits | Raytrace |
|---|---:|---:|
| Type-compatible producer-operation / consumer-pivot pairs | 3,837 | 109,994 |
| Pairs also present in the compiler's multi-stage route map | 1,132 | 1,132 |
| Applied resumptions | 3,034 | 663,403 |
| Distinct producer-rule / consumer-rule pairs | 344 | 583 |
| Distinct producer-rule, operation, consumer-rule, pivot tuples | 433 | 17,882 |
| Exact tuples observed once | 193 | 17,102 |
| Exact tuples covering 90% of resumptions | 185 | 70 |
| Distinct consumer-rule / pivot entries | 97 | 127 |
| Consumer entries covering 90% in the same workload | 35 | 18 |
| Distinct root-to-current instruction paths | 1,149 | 78,100 |
| Distinct terminal instruction paths / longest rule chain | 846 / 12 | 48,952 / 29 |

The static upper bound pairs each participating producer's publishable operation with every certified consumer entry on the same table. It includes function reads that can publish an anchor and ignores data, visibility and schedule reachability. Both static graphs contain 52 self-edges. A finite count of static edges does not bound the length of arbitrary type-compatible walks. The dynamic analysis follows each winning cursor's own ancestry through publication and forks. It counts a terminal occurrence if that applied instance had no resumed child before the schedule ended. Frequencies of terminal paths use terminal occurrences as their denominator. The results also include overlapping three-rule windows and rule-only path distributions.

On Raytrace the first 10 exact tuples cover 64.2% of all resumptions and the first 64 cover 89.1%. On sum-bits they cover 27.7% and 69.5%. The Raytrace initialization rule alone contributes 17,291 distinct tuples and just 17,589 occurrences. Removing that one producer leaves 591 distinct tuples. Rule-level grouping or entry-level parameterization avoids compiling a different block for each loader instruction offset. These are empirical concentrations for two inputs.

All resumptions cross a publication/Run boundary. On sum-bits 1,464 resumptions change source ruleset between the two adjacent rules, spread across 280 exact tuples. On Raytrace 193,423 do, across 17,666 tuples. The engine's historical `cross_stage_steps` counter counts the first visit to a ruleset in a shared root context and has a different denominator. Only 1,205 and 153,546 resumptions respectively consumed the exact original publishing instruction's physical FactRef. A current function row, forwarded receipt or reused constructor can still supply a valid anchor packet.

## What the compiler can bake

The offline Rust compiler takes an applied training profile, ranks eligible consumer-rule / pivot entries and emits fixed operand selectors for a target artifact. Rule identity uses ruleset, seminaive flag, typed slot definitions, typed query and effects. A cross-program transfer requires a unique matching target rule. The loader independently reconstructs the block against the target's certified stage entry and checks its fields, optional port and RHS template. A profile can suggest a block but cannot prove that a match exists.

The prototype covers entries with either only the anchor or one additional table atom whose complete key is available from the anchor and literals. Once ordinary scheduling and Spine routing deliver a validated visible packet, a dense rule/pivot index selects the block. It projects the consumer RHS slots from anchor columns and, where necessary, the value of one exact lookup against this Run. It preserves repeated variables, result/literal checks, visibility, the port cache, witness epochs, snapshot binding order and the deepest-origin tie break. The normal barrier applies the existing RHS template. Other shapes retain the existing matcher, and block dispatch currently requires slot transfer to be off. The block catalog contains no eclass IDs or precomputed mutable table values.

The hottest Raytrace tuple is `(153, 2, 155, 1)`, with 299,313 resumptions (45.1%). Both rules belong to `always-run`. The first publishes `Expr-size(expr) = n` for a `Get`. The second enters on `Expr-size(x) = n`, looks up `Single(x) = s`, then executes its original `set Expr-size(s) = n` RHS. Its baked input projection is `[anchor[0], port.result, anchor[1]]`. Multiple producers can switch to this same block. A missing or hidden `Single(x)` produces no answer through this entry. This consumer/pivot occurred just 44 times on sum-bits and fell outside the smaller transferred catalog.

This is profile-guided compilation after ordinary Cell/marker Bake. It does not extract an equality from the Bake Cell e-graph or reinterpret the storage of an already committed producer Cell. A whole multi-stage chain still needs a resumption point at each Run boundary, together with current mutable ports, guards, canonical keys and visibility checks. Selecting a common suffix by its checked entry ABI can cover thousands of concrete prefixes without generating thousands of effect programs.

## Held-out execution

| Training → target | Blocks | Resumptions using blocks | Coverage | Applied across different rulesets |
|---|---:|---:|---:|---:|
| sum-bits → Raytrace | 16 | 261,305 / 663,403 | 39.4% | 138,329 / 193,423 |
| sum-bits → Raytrace | 32 | 585,442 / 663,403 | 88.2% | 146,256 / 193,423 |
| Raytrace → sum-bits | 16 | 1,899 / 3,034 | 62.6% | 770 / 1,464 |
| Raytrace → sum-bits | 32 | 2,146 / 3,034 | 70.7% | 908 / 1,464 |

Each catalog was selected entirely on the other workload. All selected entries mapped uniquely in these experiments. Of the 585,442 Raytrace resumptions covered by 32 blocks, 186,292 were anchor-only and 399,150 used one live port. Runtime block entries include failed ports and competing answers. For example, Raytrace entered a block 1,613,432 times but resumed through it 585,442 times. The complete cross-ruleset counts were computed on the verified identical lineage, joining the catalog to each applied winner. Larger catalog curves in the data file are coverage projections.

| Target | Blocks from other program | Inference off → on | Paired inference Δ | Whole CLI off → on |
|---|---:|---:|---:|---:|
| sum-bits | 16 | 74.00 → 72.83 ms | +0.01 ms | unmeasured |
| sum-bits | 32 | 73.62 → 73.28 ms | -0.13 ms | 199.52 → 191.58 ms |
| raytrace | 16 | 30.97 → 30.92 s | -0.25 s | unmeasured |
| raytrace | 32 | 30.72 → 30.30 s | -0.23 s | 34.82 → 34.20 s |

Each budget used its own paired release control with the same runtime features and no catalog. Sum-bits used seven measured pairs per budget. Raytrace used four pairs at 16 blocks and three at 32. A fresh process ran each sample after one excluded warm-up per arm. The engine's inference timer covers the original schedule. Whole CLI includes artifact loading, initialization, inference and final export and was recorded for the 32-block matrix. Offline training, baking and audit instrumentation are outside the timed paths.

At 32 blocks Raytrace inference improved in all three paired trials by 178, 536 and 231 ms. Sum-bits changed direction between trials. This experiment shows a transferable dispatch mechanism, with most matching, port lookup and RHS work still present.

Every audited catalog preserves the full list of ordered pre-apply RHS digests, the final logical graph, per-Run schedule digest, chain shape and occurrence counts, maximum chain length, candidate visits and port refreshes. Both sides match the frozen reference on all 1,428 sum-bits Runs and 16,883 Raytrace Runs. Candidate visits remain 15,549 and 7,778,113. [Data, hashes, sampling order and raw timing samples](spine-shape-results.json)

## Cargo reproduction

Set `sum` and `ray` to two frozen `program.json` files. Use fresh output directories. The example also works on new artifacts emitted by `cargo cell-compile --case NAME --emit-free-join-entry --output DIR`.

```sh
m=engine/cell-runtime/Cargo.toml
export CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 CELL_CPU_FREE_JOIN=planned
export CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1
export CELL_CPU_KNOWN_RESULTS=static CELL_CPU_RANGE_GUARDS=1 CELL_CPU_SLOT_TRANSFER=off
export CELL_CPU_FD_KEYS=0 CELL_CPU_SCALAR_MERGE=0
cargo run --locked --release --manifest-path "$m" --features spine-shapes,slot-transfer-audit -- --program "$sum" --output sum-trace
cargo run --locked --release --manifest-path "$m" --features spine-shapes,slot-transfer-audit -- --program "$ray" --output ray-trace
cargo run --locked --release --manifest-path "$m" --features spine-blocks --example spine_shape_summary -- \
  "$ray" ray-trace/spine-shapes.json "$sum" sum-trace/spine-shapes.json
cargo run --locked --release --manifest-path "$m" --features spine-blocks --example bake_spine_blocks -- \
  "$sum" sum-trace/spine-shapes.json "$ray" ray-baked.json 32
cargo run --locked --release --manifest-path "$m" --features spine-blocks,spine-shapes,slot-transfer-audit -- \
  --program ray-baked.json --output ray-checked
cmp ray-trace/rhs.json ray-checked/rhs.json
cargo run --locked --release --manifest-path "$m" --features spine-blocks -- --program "$ray" --output ray-control
cargo run --locked --release --manifest-path "$m" --features spine-blocks -- --program ray-baked.json --output ray-treatment
```
