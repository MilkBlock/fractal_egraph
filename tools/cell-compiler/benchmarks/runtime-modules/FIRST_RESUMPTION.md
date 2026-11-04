# Materializing a published root on its first applied continuation

Each Run still coalesces the producer's canonical anchor keys after repair and resolves visibility before publication. The first visible winner publishes a reference-counted root handle with the logical ID, original scope and epoch. Other winning anchors of that producer share it. Its empty once-cell contains space for a pointer. A complete mutable context, including the ruleset history and port map, is allocated inside that cell when `begin_step` consumes an origin for an actual RHS binding. All sibling cursors then see the same materialization.

Until then, the consumer does exact graph lookups for ports. Matching an anchor, looking up a port, failing a guard, or proposing a binding that loses deduplication can all occur without allocating a full context. Materialized roots use the existing checked port cache. The explicit public API that requests a ready cursor still initializes it. The normal executor uses the first-resumption policy whenever stage Spine is enabled.

## Frozen Pass 3 ablation

| Input | Full contexts, first publication → first resumption | Full contexts avoided | Median inference | Faster pairs | Resumptions / longest chain |
|---|---:|---:|---:|---:|---:|
| sum-bits | 3,872 → 1,100 | 2,772 (71.59%) | 81.37 → 79.86 ms | 4/7 | 3,034 / 12 |
| raytrace | 1,488,359 → 472,906 | 1,015,453 (68.23%) | 37.19 → 35.08 s | 3/3 | 663,403 / 29 |

Logical root attempts were 4,891 and 2,481,357 on sum-bits and Raytrace respectively. The new executor allocated 3,872 and 1,488,359 shared handles for successful publications. The full-context savings exactly match the previous census of distinct published roots that never resumed. Live logical roots at the endpoint remain 1,281 and 268,621. Of those, 416 and 103,097 had a full context under the new policy.

Raytrace performed 1,104,870 port lookups on handles before first resumption. Total graph key lookups rose from 5,605,488 to 5,619,821. Port cache fills fell from 510,764 to 66,745. Historical candidate visits stayed at 9,522,225. Sum-bits performed 2,113 pre-resumption port lookups and total graph key lookups rose from 24,957 to 25,041.

| Input | Rust allocations, publication → resumption | Allocation calls saved | Reallocations, publication → resumption |
|---|---:|---:|---:|
| sum-bits | 573,929 → 565,728 | 8,201 | 46,745 → 46,745 |
| raytrace | 155,728,838 → 152,506,030 | 3,222,808 | 18,558,313 → 18,558,393 |

The heap audit counts Rust global allocator calls during schedule inference in separate diagnostic builds, excluding setup, serialization of RHS digests and final export. The allocation column combines alloc and alloc_zeroed. Realloc is separate. This counts other inference allocations along with the root handles, full contexts and their contents. The counts do not measure peak resident memory.

## Validation

Each timed sample used the same frozen program and matched the full graph endpoint and per-Run schedule digest. That digest covers logical and executed binding counts. In separate builds of both revisions, each Run was also hashed independently in source order over the actual pre-apply `(rule ID, slot/value map)` sequence. Both digest arrays matched element by element, including empty Runs. Sum-bits also matched Spine off on the initial graph and all stage graph, binding and schedule snapshots. A focused regression publishes two sibling anchors sharing one root handle and probes a second root whose guard fails. Only the shared root with an applied continuation gets a full context.

The single-thread timing protocol uses an excluded warm-up per lane, fresh processes and alternating order. Inference excludes compilation, setup, export and auditing. Compare these two revisions using the same artifacts and the [Cargo commands from the first-publication experiment](ROOTS.md#reproduce), with `9fdc920` as the first-publication control. For the separate heap and RHS check, apply [the ordered RHS instrumentation](root-rhs-audit.patch) followed by [the allocator instrumentation](resume-heap-audit.patch) with `git apply --unidiff-zero` in a temporary copy of each revision. Use the same `CHECK_RHS_DIGESTS=1` invocation as in the linked commands. Compare the `rhs.json` arrays with `cmp`. The two integers in `heap.json` are allocation and reallocation calls.

[Per-run samples, work counters, fingerprints and heap audit](resume-results.json)
