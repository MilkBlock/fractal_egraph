# Function publication at scheduled consumer boundaries

This standalone Cell CPU experiment builds on `672dbdf8`. It removes some
physical publication work across Runs, rather than only compressing saved
producer parameters. The frozen 64-unit iterative Bake catalogs are unchanged.
The mechanism is a general incremental-publication optimization, not additional
Cell-specific proof credit or a newly learned instruction combination.

## Observation frontier

The graph still performs every source-ordered function write, custom merge,
union and repair. Its function rows and epochs are current after every Run.
Spine publication and logical rule wakeups remain eager, preserving provenance,
ordered effective writes and source-schedule activation.

Derived columns, range indexes, neighborhood ports and shared-entry pending
lists need only observe the resulting version before a consumer matches. A
compiler-derived read footprint identifies those functions for each rule.
Before a Run's matching batch, the executor catches up deferred inputs of its
dirty, nonempty rules together and updates Sieve once. Source checks and until
probes activate their inputs before matching. An empty pending set, or a rule
without pending function inputs, bypasses this work. RHS/merge point reads use
the current Graph directly and do not need these derived indexes.

The pending block stores stable function-row IDs and a dirty bitmap; it does
not copy Row or Value payloads. Repeated changes to an ID retain one pending
entry. At activation the current Graph supplies the final row or a tombstone.
Each derived index already knows its previous keys, so deletion, reinsertion
and union-induced rekeying can remove old memberships without retaining an old
producer Cell. A coalesced source and its survivor retain distinct IDs.

All rules still gather bindings from the immutable matching snapshot before
any RHS in that Run executes. Changing the time of derived-index maintenance
does not expose a later RHS early. Source checks force activation at their own
observation point. An in-memory stop/resume retains the pending state.

Sieve summaries are necessary conditions and may retain obsolete positive keys.
The deferred path may omit intermediate keys that no consumer observed. Every
table dependency of the current rule is caught up before the summary can reject
a binding; removal cannot introduce a false negative. Collision tests exercise
the same boundary with deliberately uninformative fingerprints.

For H changes to U distinct row IDs between observations, expensive derived
updates fall from H to U. Capturing the changes still costs O(H) bitmap work,
and all ordered merges still execute. This does not change the complexity of
the entire inference pipeline to O(U).

## Compiler admission and maintenance cost

The emitted `scheduled_functions` certificate records the typed function-table
read footprints and the admission decisions. Loading derives it again and
rejects modified footprints. No workload or operator names participate.

Cross-key recursive propagation stays eager: if a rule reads a function at one
key and writes it at a syntactically different normalized key, the table is not
admitted. This is a conservative cost policy, not a soundness requirement.
It excludes recurring attribute propagation such as Expr-size, whose next
iteration observes almost every version in the measured workloads. Same-key
guarded updates can still be admitted.

A cold table with no column index, range index, neighborhood cache or warm entry
node needs no pending queue: its first reader builds from the current Graph.
Pending IDs use reusable vectors and one bit per function ID. Unsupported
auxiliary publication observers (`projection-semijoin` and `family-replay`)
are rejected when this option is enabled.

The first broad prototype deferred only columns. It passed the complete audits
but made Raytrace read 1,133,715 extra Rows because eager neighborhood maintenance
and later column maintenance each fetched the row. Moving all corresponding
derived maintenance to one boundary removed that duplication. The final cost
policy then reduced captured headers from 1,678,608 to 185,285, activations from
4,602 to 1,285, and peak pending IDs from 65,700 to 5,549. Run-level activation
reduces the final activation count further to 1,268. These are diagnostic
work comparisons, not speedups measured from instrumented runs.

## Actual work removed

| Metric | sum-bits, off → on | Raytrace, off → on |
|---|---:|---:|
| Column-index row updates | 5,585 → 5,426 | 2,395,590 → 2,349,965 |
| Shared-entry pending-reference appends | 6,993 → 6,834 | 2,520,358 → 2,474,737 |
| Sieve update-row processing | 30,833 → 29,162 | 8,380,801 → 7,690,160 |
| Publication Row reads | 4,750 → 4,591 | 2,231,377 → 2,185,752 |
| Residual candidate visits | 12,275 → 12,275 | 4,490,637 → 4,490,637 |

Raytrace coalesces 45,623 queued headers, including 45,558 for ExtractedExpr.
Two queued IDs remain unobserved at program completion. The remaining pending
vector capacity and bitmap occupy 183,488 bytes, excluding container headers.
The 74,662 cold headers that bypass the queue are not credited as eliminated
index updates: those indexes did not exist yet.

Cell registrations, function updates, constructor requests, logical wakeup
counts and Spine publications remain unchanged. There are still 663,403
Raytrace resumptions with a longest chain of 29, and 3,034 / 12 for sum-bits.
The contribution is fewer derived-index updates and entry-list appends, not
fewer source commits or an already-fused producer/merge/consumer RHS.

## Validation and timing protocol

Both modes match all frozen final-state, scheduled-Run, ordered RHS-input,
effective-write, resume and Spine-shape hashes. The sum-bits forced fingerprint
and Sieve-collision audit also matches. Regression checks include sequential
custom merges, mid-schedule checks, delete/reinsert, union key collisions,
in-memory stop/resume, range membership without rebuilding, neighborhood-port
refresh, and forged reader certificates. The combined lifetime-feature suite
passes 20 compiler tests and 30 runtime tests; the default build is checked
separately. This validates against the Cell control, not native egglog parity.

Timing uses ordinary release on one CPU thread, fresh processes, one excluded
warm-up per arm and five alternating pairs. Both modes enable constructor port
storage, with the previous function-parameter and two-Run experiments off. Only
the on mode enables scheduled function publication. Builds and training are
outside timing; CLI includes loading and full export.

Deferred activation is charged to the consumer's search phase; its measured
`activation_seconds` is reported separately. Capturing pending changes remains
in query-update time. Comparing only the old and new query-update phases would
incorrectly count moved maintenance as removed work.

| Pass 3 | Inference ms, off / on | CLI ms, off / on | Peak MiB, off / on |
|---|---:|---:|---:|
| sum-bits | 22.613 / 22.429 | 130.472 / 131.671 | 152.297 / 152.297 |
| Raytrace | 8,203.285 / 8,260.059 | 10,907.747 / 10,995.537 | 1,810.625 / 1,801.531 |

The final inference medians are 0.81% lower on sum-bits and 0.69% higher on
Raytrace; only two of five Raytrace pairs improve. This is not a stable speedup.
The option remains disabled by default. The earlier per-rule activation matrix
also failed to improve inference: 21.386 / 21.922 ms and 8,045.445 / 8,137.702 ms.
Both matrices are retained, rather than selecting the more favorable run.

Raytrace's final query-update median falls from 1,608.189 to 1,468.733 ms, but
111.589 ms of activation moves into search; pending capture costs another
2.020 ms, already included in query-update time. Phase medians need not sum to
the median total. RSS falls by 9.094 MiB in this matrix, but that high-water
change cannot all be attributed to the 183,488-byte pending structure.

With all other phases held fixed, eliminating even the entire measured
query-update phase would yield only about 1.24x on Raytrace. The next substantial
candidate is a compiler-recognized propagation family with directly connected
consumers, so both matching and RHS work can be removed. In particular, the
1,126,614 Expr-size publication headers in the broad survey had no cross-Run
coalescing: their consumers observed each iteration. Delaying their publication
alone cannot remove those versions. That producer/consumer fusion is still
unimplemented here; the new boundary does not claim to have delivered 2x.

Raw samples retain the complete counters. The concise summaries omit repeated
per-rule Sieve arrays, which remain available in the JSONL samples.

## Cargo reproduction

Use fresh output directories. Build the runtime separately from the compiler
for the frozen byte-level audit and timing runs.

```sh
cargo test --release --locked \
  -p eggfuse-cell-runtime -p eggfuse-cell-compiler \
  --features scheduled-functions,function-blocks,two-run-survey,origin-demand,entry-retirement,view-ports,storage-metrics

cargo build --release --locked -p eggfuse-cell-runtime \
  --features scheduled-functions,port-frames,view-ports,storage-metrics \
  --bin eggfuse-cell-runtime --example bench_deferred_incidence
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/scheduled-pairs \
  out/sum-bits/program.json out/raytrace/program.json 5 --scheduled

cargo build --release --locked -p eggfuse-cell-runtime \
  --features scheduled-functions,port-frames,view-ports,storage-metrics,slot-transfer-audit,spine-shapes
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/scheduled-audits \
  out/sum-bits/program.json out/raytrace/program.json 0 --audit-scheduled

cargo run --release --locked -p eggfuse-cell-compiler \
  --features scheduled-functions,port-frames,view-ports -- bench \
  --program out/sum-bits/program.json \
  --reference-program out/sum-bits/program.json \
  --port-frames on --scheduled-functions on --repeat 5
```
