# Publishing Spine anchors and loading ports on demand

On the frozen Raytrace Pass 3 schedule, every tested policy executes 3,785,196 RHS bindings. Each produces 820,439 logical facts, 1,676,336 effective function updates, 226,523 effective unions and 2,798,026 distinct delivery events. The routed Spine creates 3,741,283 physical Cells compared with 3,487,688 when Spine is off. Context maintenance and frame preparation add cost even for the same source-level mutations.

Previous full capture installed 12,651,676 entries in consumer context fact maps on Raytrace. Consumers attempted 65,928 reads of these maps. The two counts refer to operations and can revisit a key. The old receipt mode removed much of the repeated graph lookup cost while retaining the eager capture and map updates.

## Publication protocol

1. At program load, derive the anchor-carrying RHS operation indices for each producer from the validated functional-key entry plans. A participating rule retains both writes and RHS reads of anchor tables that previously published a cursor.
2. On a completed RHS, retain the table, input key, cursor and optional action witness for those anchors. The matcher can omit the LHS receipt vector from its query workspace and from each completed binding. Ordinary RHS effects stay in source order.
3. After repair, canonicalize queued keys if unions have occurred since capture. Coalesce by table and complete key within that Run. The last producer at a canonical key owns the next cursor, including when distinct pre-repair keys become equal.
4. Follow only same-key forwarding and storage relocation. Check table and visibility on the final witness. A missing or retired receipt falls back to exact keyed lookup, including when a replacement could have appeared in the same Run. Stage audits additionally decode and verify the receipt key.
5. At an eligible consumer, populate non-pivot ports on first demand using the exact graph key. A later use rechecks the witness's table, current key and visibility. Missing ports are read again when needed.

Spine remains opt-in. Within enabled Spine, RHS anchor receipts now default on. Explicit `CELL_CPU_STAGE_SPINE_RECEIPTS=0` selects exact keyed anchor publication. Timing the new policy with keyed lookup alone is outside the paired matrix. Its diagnostic counters are included in the raw results.

## Same-input comparison

| Spine policy | sum-bits median | Raytrace median | Raytrace publication | Raytrace RHS |
|---|---:|---:|---:|---:|
| Previous full capture | 99.78 ms | 47.20 s | 9.47 s | 13.18 s |
| Previous full capture + receipts | 98.90 ms | 45.98 s | 6.57 s | 13.82 s |
| Anchors + on-demand ports | 81.03 ms | 36.31 s | 1.82 s | 10.71 s |

The publication column measures the Spine publisher within the broader query-update phase. Each lane received a warm-up and rotated through seven measured rounds on sum-bits or three on Raytrace. All used the same frozen marked artifact, a fresh process per run, one Apple M3 Max thread and release builds. Compilation, initialization, full export and validation are outside inference. The first two policies use the parent engine. The third uses this branch. Spine off and new keyed-only publication were separate single-run work-count controls on both inputs. On Raytrace they recorded 33.97 s and 39.43 s respectively. The table uses only the repeated lanes.

| Raytrace work | Previous full capture | Anchor publication |
|---|---:|---:|
| Eager context fact-map writes | 12,651,676 | 0 |
| Successful anchor installations | 3,704,604 | 2,302,142 |
| Capture graph lookups | 6,002,351 | 29 |
| Graph lookups to refresh consumer ports | 1,532,493 | 1,561,633 |
| Live port references reachable from anchors at the endpoint | 1,739,516 | 108,046 |
| Historical candidate visits | 9,522,225 | 9,522,225 |
| Resumptions / maximum chain | 663,403 / 29 | 663,403 / 29 |

Raytrace queued 3,704,657 anchor observations, which became 2,302,171 distinct within-Run keys after repair. Of those, 2,302,142 resolved directly and 29 required an exact lookup. There were 42,761 same-key redirects. Consumers filled 510,764 cache entries on demand. The live reference count describes retained port references at the final graph and does not measure peak process memory. On sum-bits, 34,508 eager context writes became 8,414 anchor installations and 1,849 demand fills, with all 3,034 resumptions and maximum chain depth 12 preserved.

## Verification and Cargo commands

Each timed endpoint and per-Run scheduling and binding-count digest matches the parent. The initial graph and all 1,428 sum-bits Run graph, ordered binding and schedule snapshots match the current binary with Spine off. A separate untimed build hashed ordered `(rule_id, slot_values)` pairs from the original RHS patch. Every one of Raytrace's 16,883 per-Run digests matches the archived parent audit. Intermediate full Raytrace graphs are outside this audit. A focused test exercises two producers whose keys merge, a redirected receipt, deletion before publication and both receipt and keyed modes.

```sh
cargo cell-compile --case sum-bits --emit-free-join-entry --output out/anchor-sum
cargo cell-bench --program out/anchor-sum/program.json --lane free-join-planned --modules stage-spine --repeat 7 --verify-stages
cargo cell-compile --case raytrace --emit-free-join-entry --output out/anchor-ray
cargo cell-bench --program out/anchor-ray/program.json --lane free-join-planned --modules stage-spine --repeat 3
```

The Cargo bench worker clears ambient engine settings. To reproduce a receipt ablation with the runtime CLI, set the flags explicitly and use a new output directory. Compare at `51cd290` and on this branch using the same compiled artifact. Set `CELL_CPU_STAGE_SPINE_RECEIPTS` to 0 or 1 for the two policies at each revision.

```sh
CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1 \
CELL_CPU_FREE_JOIN=planned CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 \
cargo cell-run --program out/anchor-ray/program.json --output out/anchor-ray-trial
```

[Per-run timings, effects, phase medians and validation fingerprints](anchor-results.json)

The next ablation defers root allocation until a visible anchor wins publication. [Root allocations and ordered RHS audit](ROOTS.md)
