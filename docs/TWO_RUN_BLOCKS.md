# A checked two-Run block with a shared parameter row

This standalone Cell CPU experiment builds on `edce37c2`. The first phase keeps
its compact producer row. At the second phase's original scheduled occurrence,
the engine verifies a compiler-derived projection of every retained consumer
parameter, then appends the consumer fact state to that existing block. Both
logical producer IDs remain valid views. The consumer has no independent
parameter row. Matching, RHS effects, global indexes and publication remain
source-scheduled.

## Selecting a real combination

The diagnostic tracks the actual constructor anchor behind applied Spine
continuations. It only counts consumers that actually create a compact record.
Disjoint estimates reserve each producer at most once; they do not count the
same record in several proposed pairs.

| Funnel | sum-bits | Raytrace |
|---|---:|---:|
| Applied Spine resumptions | 3,034 | 663,403 |
| Function-row anchor | 87 | 307,940 |
| Unsupported consumer RHS | 1,124 | 175,136 |
| No compact source producer | 594 | 80,103 |
| Prepared constructor sources | 1,229 | 100,224 |
| Consumers creating new compact ports | 450 | 45,884 |
| Disjoint pairs sharing at least two values | 228 | 22,495 |

The compiler narrows these to consumers whose complete retained input tuple is
provided by producer input slots or literals, and whose new RHS fact is one
constructor Let. It ranks observed calls times consumer-parameter count and
selects one combination. Transfer uses the existing typed rule signature,
including the source ruleset, query and effects; ambiguous matches are rejected.
The target derives the proof again. Profile frequency supplies admission only.

Both independent profiles select the same shape:

```text
A: Single(e)        -> Get(Single(e), 0) = e
B: Get(tuple, i)    -> Add-Gets(tuple, tuple, 0)

B's tuple = A's Single(e) parameter
B's i     = literal 0
B's result parameter = A's e parameter
```

Neither operation name nor rule number is hardcoded in execution or selection.
The sum-bits profile is transferred to Raytrace, and the Raytrace profile to
sum-bits. The original 64-unit iterative catalogs are unchanged; the new
`two_run_blocks` artifact is an additional, checked storage-composition plan.

## State and lifetime contract

Before attaching the second phase, the engine checks the source's live fact,
template and instruction, a strictly earlier Run, complete consumer execution,
the exact stored-slot set, canonical equality of every projected value, and
absence of parent links. Occupied or incompatible second-phase ports fall back
to ordinary compact storage. Guards do not mutate the graph.

The logical-ID directory distinguishes owner and child views. Child states are
stored densely, with a small owner-to-child index; their parameters are read
from the owner row or the shared literal. Moving an owner updates the child's
owner index. Moving a child updates its directory address and owner link.

If the owner retires first, a surviving consumer is reconstructed as an ordinary
compact producer with its original ID, parameters, fact state, epoch and depth.
If both views are dead, both are released. If the consumer dies first, its state
is removed while the owner remains. Sparse child storage is trimmed only at
the original GC barrier and only after its occupancy drops below one half.
These transitions prevent parameter sharing from retaining an otherwise dead
owner indefinitely.

The non-mutating survey has its own `two-run-survey` diagnostic feature. Timed
builds do not carry survey anchors in every Cursor. Execution requests only the
selected consumer's existing query witness; that maintenance is included in
the on-lane timing.

## Actual work and representation

| Metric | sum-bits | Raytrace |
|---|---:|---:|
| Applied two-phase attachments | 48 | 7,310 |
| Consumer parameter values omitted at attachment | 144 | 21,930 |
| Owner-first detachments | 25 | 3,574 |
| Fused children still live at finish | 23 | 3,736 |
| Primary parameter rows, off → on | 1,432 → 1,409 | 301,455 → 297,719 |
| Stored parameter values, off → on | 7,630 → 7,561 | 2,536,195 → 2,524,987 |

Logical producer views remain 2,875 / 687,913 when full Cells, compact views and
embedded writers are counted together. Child metadata and fact states are
reported separately; fewer primary rows do not mean fewer logical facts.
Constructor requests, effective writes, union requests, repair work, candidate
visits and publication counts are unchanged. This experiment has not removed
global graph maintenance.

The first dense child-column version allocated space for empty child ports.
The final sparse version reduces Raytrace's measured vector-capacity payload
by 191,568 bytes in the timed configuration; sum-bits grows by 976 bytes. These
figures include the child columns but exclude allocator metadata and are not RSS.

## Paired inference and complete CLI

Five measured fresh-process pairs follow one excluded warm-up per arm. The
order alternates. Both arms enable compact ports; only the on arm enables
two-Run fusion. Builds use ordinary release, one thread, no PGO and no survey
instrumentation. Compilation and training are outside the process measurements.

| Pass 3 | Inference ms, off / on | CLI ms, off / on | Peak MiB, off / on |
|---|---:|---:|---:|
| sum-bits | 21.292 / 22.400 | 121.795 / 127.383 | 152.359 / 152.156 |
| Raytrace | 8,343.862 / 8,342.359 | 10,757.929 / 10,836.943 | 1,801.813 / 1,791.703 |

The mechanism works, but this matrix does not establish an inference or CLI
speedup. It remains off by default. In particular, the small capacity change
cannot account for the entire sampled RSS difference on this unisolated host.
The opportunity funnel explains the limited scope: most resumptions involve
function anchors or RHS effects excluded from this constructor-only prototype.
More meaningful inference gains require merge-aware function ports and/or
removal of internal global indexing/publication work, with their own contracts.

## Validation and Cargo entry points

Both workloads match the frozen final graph, every scheduled Run, ordered RHS,
effective writes and complete Spine trajectory. The artifact identity changes
when the checked catalog is attached; only that header is normalized in the
Spine comparison, with the raw and normalized hashes both retained. Tests cover
owner-first and child-first retirement, child relocation, owner relocation,
guard failure and certificate tampering.
The sum-bits ordered audit also matches under forced fingerprint and Sieve
collisions. The final regression passed 20 compiler tests and 28 runtime tests,
including the existing memory-lifetime options; the prior port-only build was
checked independently.

```sh
# Survey the existing frozen compact-port execution; repeat for both cases.
cargo run --release --locked -p eggfuse-cell-runtime \
  --features two-run-survey,view-ports,storage-metrics -- \
  --program out/sum-bits/program.json --output out/sum-survey \
  --port-frames --two-run-survey

# Cross-workload training: reverse the inputs for the other direction.
cargo run --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks --example bake_two_run -- \
  out/sum-bits/program.json out/sum-survey/report.json \
  out/raytrace/program.json out/two-run-ray.json

cargo build --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks,view-ports,storage-metrics
cargo run --release --locked -p eggfuse-cell-runtime \
  --example bench_deferred_incidence \
  --features two-run-blocks,view-ports,storage-metrics -- \
  target/release/eggfuse-cell-runtime out/two-run-pairs \
  out/two-run-sum.json out/two-run-ray.json 5 --two-run
```

Use fresh output directories. For independent ordered audits, add
`slot-transfer-audit,spine-shapes,receipt-slots` to the two final builds and use
`0 --audit-two-run`. The compiler bench accepts `--two-run-blocks on|off`; small
fixtures can use `--reference-program` to verify every Run before timing.
Raw profiles, catalogs, repetitions, input hashes and binary hashes accompany
this document in `two-run-blocks/`.
