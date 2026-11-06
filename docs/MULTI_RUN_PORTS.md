# Multi-Run parameter and result ports

This opt-in standalone Cell CPU implementation extends `ce64ea3a` on
`codex/multi-run-port-blocks`. It supports bounded chains of two through eight
scheduled phases, reads earlier constructor results as ports, and installs
shared RHS inputs before execution. A third phase can also complete an
existing constructor directly from an earlier fact port, without a global key
lookup. Mutable function writes and general merge blocks remain outside this
constructor-only storage contract.

## Compiler and execution

The typed compiler emits `multi_run_blocks`: closed producer/consumer edges in
which the selected constructor atom supplies every consumer input. Inputs can
refer to producer parameters, literals, or constructor result slots. Static
aliases and Copy definitions are normalized first. Loading derives every edge
again and rejects a changed projection or result-slot claim. The frozen
sum-bits and Raytrace programs each yield 102 edges, including 68 with producer
result slots. This analysis uses the whole typed program, without PGO or a new
Bake training run; the existing 64-unit iterative catalogs remain unchanged.
The two independently compiled edge catalogs are byte-identical for the shared
rule program; `multi-run-ports/catalog.json` stores one copy. Workload initial
graphs and input artifacts remain distinct, with both input hashes recorded.

These are parameter-closure certificates. The existing matcher still confirms
the complete LHS and its guards, gathers all bindings before RHS execution, and
preserves their order. Additional atoms are not assumed true merely because
their variables are available. The rule's matched receipt selects the actual
producer; candidate plans are grouped by pivot and binary-searched by producer
template/instruction.

At the first occurrence of a concrete root-slot layout and transition, the
engine specializes and caches its projection. Parameter references are flattened
to root columns or literals. Result references identify the earlier phase and
its defining slot. They resolve a logical result identity, rather than decoding
an intermediate Row. The compiler plan is shared; each instance keeps its own
logical IDs, epoch and fact state.

Before an RHS borrows inputs, the engine checks the live visible source fact,
producer instruction, an earlier Run, the complete binding projection, a free
successor position and the phase budget. Its temporary frame then omits those
input entries. The existing binding vector was still produced by matching;
this does not claim to have eliminated that allocation. A root that itself
borrows an external constructor parent can be made owned once, with copied
parameters and the released parent hold counted separately.

During layout specialization, symbolic constructor keys are composed through
the available phase projections. An equal operator and equal symbolic key can
identify an earlier output port. At the consumer's original RHS occurrence,
the engine checks that fact is still live and visible, reads its actual committed
result, and forwards the original receipt. Deleted, subsumed or unavailable
ports use ordinary construction. The new counter records a discharged global
constructor request; it does not omit the logical RHS application or its unions.

## Representation and lifetime

The directory bit now distinguishes a parameter owner from a phase view; it no
longer means "phase two". Views carry an explicit phase, source logical ID,
compiled layout and logical successor ID. Each source currently has one shared
successor. Additional consumers retain the ordinary path. The maximum of eight
phases is a bounded execution policy, not an unrolling of every saturate path.

Root-row movement repairs the owner offsets of its complete descendant chain.
View movement only updates the moved logical ID's directory entry. Root or
middle retirement first snapshots every surviving descendant's projected inputs,
including earlier result identities, and detaches from the tail. Survivors keep
their original IDs as ordinary compact owners. This may give up suffix sharing,
but avoids pinning an otherwise dead root or leaving dangling parameter ports.
Capacity and guard failures use the ordinary representation.

Regression fixtures verify new three-phase records, root/middle/tail retirement,
root and view relocation, an eight-phase recursive chain that restarts at its
budget, same-key completion, different-key fallback, hidden/deleted outputs,
and forged certificates. The completion fixture retains a separate fact in the
root while deleting the reusable fact, so it tests the receipt guard separately
from whole-root reclamation.

## Workload results

Three modes use one binary and one artifact per workload:

* `legacy2`: the previous selected two-phase catalog and late attachment.
* `early2`: the 102-edge catalog, early RHS input projection, depth limited to two.
* `early8`: the same catalog and early projection, depth limited to eight.

The first comparison includes both expanded compiler admission and early
execution. Only `early2` versus `early8` isolates the deeper phase budget.
The legacy arm uses the generalized store; its timing is not a rebuild of the
old `ce64ea3a` binary. Its original 48 / 7,310 attachment counts are preserved.

| Metric | sum-bits legacy2 / early8 | Raytrace legacy2 / early8 |
|---|---:|---:|
| Installed shared phase views, cumulative | 48 / 94 | 7,310 / 9,879 |
| Primary parameter rows at finish | 1,409 / 1,363 | 297,719 / 295,216 |
| Stored root parameter values at finish | 7,561 / 7,411 | 2,524,987 / 2,516,140 |
| Early input projections, phase 2 | 0 / 163 | 0 / 10,636 |
| Early input projections, phase 3 | 0 / 0 | 0 / 832 |
| Direct completions from an earlier phase | 0 / 0 | 0 / 832 |
| Global constructor requests | 11,502 / 11,502 | 2,065,807 / 2,064,975 |
| Global complete-key lookups | 19,352 / 19,352 | 4,423,918 / 4,423,086 |

The 832 third-phase Raytrace calls all reuse an existing constructor in the same
block. They now consume that fact port directly. They do not create new retained
third-phase records; neither control needed a new logical node for these calls.
Persistent chains in these two workloads therefore still reach only phase two.
Persistent depth three and depth eight are exercised by the regression fixtures,
not claimed as observed workload coverage.

The two early modes have identical endpoint storage. The additional Raytrace
work discharged by `early8` is precisely the 832 lookups/requests. Existing
iterative handoffs remain 30,896 on Raytrace. Full Cell registrations, logical
fact creation, function writes, original publication and total retained producer
views do not fall. Matching counts are unchanged.

Smaller parameter counts do not imply smaller overall storage in this prototype.
The reported root-slab plus phase/layout vector capacities increase by 18,788
bytes on sum-bits and 165,152 bytes on Raytrace from legacy2 to early8. Those
figures exclude layout-map nodes and allocator overhead. New view metadata and
cached symbolic exports currently outweigh the saved parameter payload. Root
parent flattening occurs in the fixtures, but zero times in these workloads.

## Validation and measurements

All three modes match the frozen full graph, every scheduled Run, ordered RHS
inputs, effective writes, resume stream and complete Spine-shape stream. Only
the program-identity header is normalized for the added checked catalog; raw
and normalized digests are retained. Resumptions and longest chains stay
3,034 / 12 on sum-bits and 663,403 / 29 on Raytrace. The sum-bits forced hash
and Sieve-collision audit also passes. The regression passes 21 compiler tests
and 28 runtime tests, including the existing lifetime options; default and
RHS-work-audit feature builds are checked separately. These checks compare the
Cell executor with its control, not an independent native-egglog implementation.

Five fresh-process measurements per mode follow one excluded warm-up. Mode
order rotates. The build is ordinary release, single-threaded, without PGO.
Build/training are excluded; complete CLI includes artifact loading and export.

| Pass 3 / mode | Inference ms | CLI ms | Peak MiB |
|---|---:|---:|---:|
| sum-bits / legacy2 | 21.973 | 133.853 | 152.500 |
| sum-bits / early2 | 22.353 | 132.696 | 152.578 |
| sum-bits / early8 | 22.775 | 131.434 | 152.547 |
| Raytrace / legacy2 | 8,283.831 | 10,945.204 | 1,797.031 |
| Raytrace / early2 | 8,498.085 | 11,143.951 | 1,784.453 |
| Raytrace / early8 | 8,246.917 | 11,042.331 | 1,804.063 |

Relative to legacy2, early8 is 3.65% slower in sum-bits inference and 0.45%
faster in Raytrace inference; Raytrace CLI is slower. The difference between
early2 and early8 must not all be attributed to 832 saved requests: process
variation is much larger than the work reduction, and their endpoint storage
is almost identical despite the RSS difference. No reliable overall speedup or
memory reduction is established, and the new mode stays disabled by default.

This completes the first multi-phase mechanism, not a general replacement for
matching or function/merge execution. The remaining coverage limits are a
single shared successor, one non-eliminated constructor output per consumer,
and all consumer inputs supplied by one matched atom. A broader next step is
to close a consumer LHS jointly from several available block ports and to add
versioned function/merge ports, while retaining source-scheduled visibility.

## Cargo reproduction

The existing `two-run-blocks` Cargo feature gates both the compatible legacy
path and the new implementation. Both runtime modes remain off by default.

```sh
cargo test --release --locked \
  -p eggfuse-cell-runtime -p eggfuse-cell-compiler \
  --features two-run-survey,view-ports,storage-metrics,origin-demand,entry-retirement

# New typed compilations emit the multi-run catalog automatically with this feature.
cargo run --release --locked -p eggfuse-cell-compiler \
  --features two-run-blocks -- compile --source input.egg --output out/compiled

# Extend each frozen artifact, preserving its previous two_run_blocks catalog.
cargo run --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks --example bake_multi_run -- \
  out/two-run-sum.json out/multi-sum.json
cargo run --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks --example bake_multi_run -- \
  out/two-run-ray.json out/multi-ray.json

# Build the standalone runtime separately from the compiler for consistent JSON.
cargo build --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks,view-ports,storage-metrics \
  --bin eggfuse-cell-runtime --example bench_deferred_incidence
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/multi-pairs \
  out/multi-sum.json out/multi-ray.json 5 --multi-run

cargo build --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks,view-ports,storage-metrics,slot-transfer-audit,spine-shapes
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/multi-audits \
  out/multi-sum.json out/multi-ray.json 0 --audit-multi-run

cargo run --release --locked -p eggfuse-cell-compiler \
  --features two-run-blocks,view-ports -- bench \
  --program out/multi-sum.json --reference-program out/multi-sum.json \
  --multi-run-blocks on --port-block-depth 3 --repeat 5
```
