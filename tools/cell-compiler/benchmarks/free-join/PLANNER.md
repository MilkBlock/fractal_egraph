# Costing Free Join before entry delivery

The opt-in `free-join-planned` lane uses the projection descriptors emitted by the Rust compiler. For an unchanged target table, consumers can reuse a projected lookup across Runs. On single-column equalities a consumer can also drive from a small follower key domain. Both decisions use the typed rule and current graph statistics. They apply to previously compiled artifacts through the existing plan derivation path.

## Where the probes went

| Physical entry index probes | Raytrace | sum-bits |
|---|---:|---:|
| Entry lane with per-Run cache | 12,350,778 | 24,592 |
| Of those probes, returned empty | 9,352,941 | 15,694 |
| Of those probes, repeated the same key at the same target version | 8,461,053 | 6,578 |
| Cross-Run version cache only, unlimited diagnostic | 3,889,725 | 18,014 |
| Cross-Run version cache only, bounded implementation | 4,339,453 | 19,598 |
| Bounded cache and reverse driving | 3,155,529 | 16,537 |

The empty and repeated categories overlap. For Raytrace the bounded cache saves 8,011,325 physical lookups. Driving from the smaller side saves another 1,183,924 with the same capacity policy. For example, a single `Uop` projection returned empty on 1,246,825 of 1,246,872 physical probes. The target averaged about three witnesses at those probes. On the `Single` projection, 1,136,938 of 1,291,748 probes repeated the same fingerprint tuple at an unchanged target version. The first shape favors enumeration from the follower. The second offers cross-Run reuse.

A cached result is a fingerprint-based superset of target references, including the empty set. The table query version guards membership through writes, visibility changes, repairs and forwarding. The executor checks the current delta owner on every singleton use and still verifies actual row values. Each projection clears on a version change or when its key count exceeds the sum of the current driver and target cardinalities, with a floor of 64. This limits retained negative keys per projection. It does not bound process RSS or total referenced rows.

For reverse driving, select the eligible follower with the fewest distinct indexed keys, using witness count until its index exists. Let `K` be that estimate. Require more than `max(8, 4*K)` current delta packets. Per-node epoch counts give that test without scanning every packet. Enumerate follower fingerprints against the driver's column index. Fall back if follower keys plus matching driver references exceed the same budget. Filter the resulting references against the shared node and consumer watermark, sort by the original epoch/witness order, then run the ordinary Sieve, remaining entry checks and RHS. Collisions can only widen the candidate domain.

| Work inside Raytrace | Entry | Planned |
|---|---:|---:|
| Entry rows reaching projection checks | 14,208,396 | 8,341,904 |
| Physical entry probes | 12,350,778 | 3,155,529 |
| Entry cache hits | 2,490,930 | 5,758,914 |
| Reverse plans completed / with no eligible packet | 0 | 11,761 / 6,682 |
| Driver references collected by reverse plans | 0 | 795,801 |
| Epoch bins read for the planning decision | 0 | 91,185 |
| Seeds delivered to consumers | 3,437,773 | 3,437,773 |
| Historical candidate visits | 9,522,225 | 9,522,225 |
| Full row decodes in the graph matcher | 7,862,010 | 7,862,010 |

Reverse attempts also read about 0.84 million follower keys on Raytrace. The exact prefix of an over-budget hash walk varies across processes, so the Cargo worker reports a range for this counter. The proof-of-concept which counted delta packets individually needed 13,270,481 header iterations for the same decisions. The 74.45% reduction applies to physical forward entry probes. Sorting, cache lookups, epoch maintenance and reverse enumeration are included in the inference timings.

## Native planner and timing

The native reference executes the same marked source on the real egglog runtime at `egglog-baseline`. Ordinary multi-atom rules default to the variable-oriented `Gj` strategy, and single-atom and internal rebuild rules select `MinCover`. The native implementation constructs tree-decomposition bags where applicable, passes projected materializations between them and can reorder eligible remaining stages using current subset estimates. Cell currently chooses a physical side for one connected equality at a delta pivot. The rest of the entry probes still follow the compiler's constrained-column order. A candidate next step is to cost the remaining subatoms using the domains produced by the first probe. [Native planning](https://github.com/MilkBlock/eggfuse/blob/62ae6baa5539af4468a8f1a94d6e108de4dcd2df/egglog/core-relations/src/free_join/plan.rs) · [Native execution](https://github.com/MilkBlock/eggfuse/blob/62ae6baa5539af4468a8f1a94d6e108de4dcd2df/egglog/core-relations/src/free_join/execute.rs)

| Frozen Pass 3 | Native default | Cell entry | Cell planned | Probes saved | Planned faster pairs |
|---|---:|---:|---:|---:|---:|
| Raytrace | 21.466 s | 33.043 s | 31.092 s | 74.45% | 3/3 |
| sum-bits | 53.61 ms | 69.42 ms | 69.66 ms | 32.75% | 2/7 |

For each case the independent Cell CPU executor and the native runner used one Apple M3 Max thread and the same marked source. The two Cell lanes used the same release binary and frozen artifact. Each lane started with an excluded warm-up, then the measurement order rotated across three fresh processes per lane on Raytrace and seven on sum-bits. Cell times cover schedule execution. Native times cover the commands from the first Run following bootstrap, with work counters compiled out. Both exclude compilation and final export. The historical eight-case matrix used a separate timing session.

| Raytrace phase median | Cell entry | Cell planned | Native default |
|---|---:|---:|---:|
| Search | 16.95 s | 15.12 s | — |
| RHS | 8.81 s | 8.76 s | — |
| Query index updates | 4.42 s | 4.41 s | — |
| Repair | 1.84 s | 1.83 s | — |
| Delivery and GC | 0.92 s | 0.92 s | — |
| Native search and apply | — | — | 15.56 s |
| Native merge / rebuild | — | — | 0.39 / 4.94 s |

The entry change saves 1.95 s end to end on Raytrace. The median search phase drops by 1.83 s. The residual solver still examines 6,522,466 candidates, the graph matcher decodes 7,862,010 rows and the executor processes 3,785,196 bindings. Cell still takes 9.63 s longer than the native default. The phase timers have engine-specific boundaries and do not fully cover either wall timer.

| Native operator work, counted separately from timing | Raytrace | sum-bits |
|---|---:|---:|
| Index keys enumerated | 27,157,348 | 35,418 |
| Projected rows scanned | 4,480,095 | 8,230 |
| Materialized keys or rows | 1,211,629 | 662 |
| Index probes, including misses | 24,279,937 | 30,864 |
| Bindings offered to action buffers | 6,816,917 | 9,114 |

Native counters cover the full join executor including internal rules. A Cell physical entry probe counts one projected lookup before seed delivery. The native operator total on Raytrace is 57,129,009 when summing the first four rows. The action-buffer counter measures a different boundary from Cell's scheduled bindings. Neither metric counts committed graph mutations.

A second instrumented run counted 9,849,577 `Intersect`, 1,886,304 `FusedIntersect` and 141,028 `FusedIntersectMat` dispatches on Raytrace. Sum-bits dispatched 18,456, 5,767 and 379 respectively. The first five work counters reproduced exactly. This directly confirms that intersection, fused covers and materialized stages all execute under the default policy.

Forcing every ordinary rule to use native `MinCover` caused both marked sources to panic during rule registration in `BucketQueue::pop_min` at `free_join/plan.rs:1279`. The corresponding `PureSize` experiment also stopped during planning at line 1426. Both sites look up the current atom's variable in the planning context. The table reports the tag's successful default schedule. It supplies no timing for either forced policy.

## Reproduce

```sh
cargo cell-compile --case sum-bits --emit-free-join-entry --output out/planner-sum
cargo cell-bench --program out/planner-sum/program.json --lane free-join-entry --repeat 7
cargo cell-bench --program out/planner-sum/program.json --lane free-join-planned --repeat 7 --verify-stages

cargo cell-compile --case raytrace --emit-free-join-entry --output out/planner-ray
cargo cell-bench --program out/planner-ray/program.json --lane free-join-entry --repeat 3
cargo cell-bench --program out/planner-ray/program.json --lane free-join-planned --repeat 3
```

The native runner, count definitions and tag extraction commands are in the [native benchmark instructions](README.md). Pass the two corresponding `source_sk_mk.egg` files to that runner. Apply [native-stages.patch](native-stages.patch) after the count patch to reproduce the three extra stage dispatch counters. The separate [MinCover](min-cover.patch) and [PureSize](pure-size.patch) patches each change one builder default and reproduce the registration failures when applied individually in a temporary tag checkout. Use `git apply --unidiff-zero` for these patches. Compile out `EGGLOG_COUNT_FREE_JOIN` for timings.

For the diagnostic upper bound, apply [entry-reuse-audit.patch](entry-reuse-audit.patch) with `git apply --unidiff-zero` to a separate checkout of `b9a97a6`. Run the `entry` engine with shared node scheduling, Sieve and column intersection enabled. Each physical miss updates a per-projection record of probes, empties, singletons, returned references, same-version repeats, cumulative target cardinality and largest same-version key set. The bounded-cache ablation skips the call to `FreeJoin::reverse` in the planned lane. Neither diagnostic contributes timing samples.

The two Cell lanes agreed on the initial graph and every graph, ordered binding and schedule snapshot for all 1,428 sum-bits Runs. Raytrace matched on every measured full endpoint, the scheduling and binding counts of all 16,883 Runs, and a separate untimed digest of the ordered RHS input records per Run. That digest includes the rule, values, epoch, parent, continuation, known facts, receipts and witnesses. Raytrace intermediate graph snapshots and native graph equivalence are outside this audit. The cyclic-join regression also exercises forced fingerprint collisions, disjoint keys, a late insertion and the budget fallback.

[Per-run timings, phase medians, counters and fingerprints](planner-results.json)
