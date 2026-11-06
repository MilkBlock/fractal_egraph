# Checked function completion ports

This standalone Cell CPU experiment extends `ce64ea3a` beyond constructor-only
blocks. A completed single-function RHS can retain a typed, immutable port block
for later source-scheduled consumers, including functions with old-value,
min/max, and custom merge contracts. It remains opt-in. The frozen 64-unit
iterative Bake catalogs and both workload sources are unchanged; this is a
static compiler analysis, without profile admission or PGO.

## Compiler and lifetime contract

The compiler accepts a final function Set preceded only by Copies and primitive
computations. It retains the normalized key and original proposal, plus every
source column requested by the validated local continuation plans. Other query
inputs and intermediate snapshots are dead after completion. Required scalar
ports must have Eq, i64, bool, Unit or f64 representations, with at most 64 ports.
Unsupported effects and representations keep the ordinary writer.

The `function_blocks` certificate records the destination, instruction position,
typed retained slots, omitted slots and inputs required to execute the RHS.
The loader derives it again from the typed recipe and executable continuation
plans. A changed slot projection is rejected. Names and workload-specific rule
numbers do not participate in admission.

For a proposal `p` and an existing value `v`, the row receives `merge(v, p)` in
the original order. The producer ports retain the original RHS parameters and
proposal, even if a custom merge returns another value or rebuild later changes
the row. They never borrow the current mutable row value as an immutable input.
No-op writes retain the existing no-frame path. This does not suppress a merge,
coalesce two effective updates, or reorder publication.

Two owned words fit inline. Wider snapshots use an exact-sized word buffer,
reused when the previous unretained producer has the same width. Slot IDs and
types belong to the shared compiler plan, rather than every producer instance.
A presence mask distinguishes owned ports from ports borrowed from a retained
constructor parent. Copies are evaluated through the original checked recipe;
Eq values are canonicalized on access, and float bits are preserved exactly.

If another producer retains an old function producer, replacing or deleting
the function origin first restores that old version's demanded slots to an
ordinary immutable Cell. Its existing parent reference transfers with it.
Otherwise the old word buffer can be reused. Consumers therefore never observe
the next proposal through a stale source reference.

For a small complete projection already present in the matched input tuple,
the single-write path can own those ports and omit the constructor-parent hold.
Regression fixtures exercise this path. Neither frozen workload uses it, so it
is not credited with any Cell-count or timing benefit in the measurements below.

## Coverage and costs

The accompanying `function-blocks/` directory contains raw paired samples,
binary/input hashes and independent ordered audit digests.

| Counter | sum-bits | Raytrace |
|---|---:|---:|
| Statically certified templates | 59 | 59 |
| Function block installations | 601 | 1,285,076 |
| Ordinary writer fallbacks | 153 | 62,982 |
| Omitted snapshot slots, cumulative | 391 | 2,724,373 |
| Compact function producers live at finish | 435 | 82,127 |
| All embedded function producers live at finish | 490 | 85,989 |
| Wide buffer allocations | 491 | 164,643 |
| Wide buffer reuses | 0 | 450,138 |
| Earlier-Run producer-context requests | 74 | 23,524 |
| Reads from compact function parameters | 12 | 26 |

Installations are effective producer versions, not distinct logical facts or
new Cell registrations. Context requests are not applied Spine resumptions.
The extremely low parameter-read count is useful evidence: most current Spine
execution consumes the committed function row and its existing cursor, rather
than these saved producer parameters. Thus broad storage coverage alone cannot
establish broad elimination of inference work.

Endpoint parameter heap capacity falls from 82,104 to 33,648 bytes on sum-bits
and from 12,428,184 to 4,238,128 bytes on Raytrace. The experimental parameter
tag also grows each writer header by eight bytes relative to the uncompiled
feature. Charging that cost across all live writers leaves a scoped net saving
of 44,536 / 7,502,144 bytes in writer headers and parameter payloads. These
totals exclude allocator metadata and compiler-plan storage; they are not RSS.
The same-binary off/on matrix includes the larger headers in both arms.

Cell registrations, logical facts, function updates, candidate visits, merge
calls, global publication and retained producer counts remain unchanged on the
two frozen workloads. This implementation has not yet fused a function write
and its scheduled consumers into one state transition, or internalized their
global index/publication work. Custom merge-generated constructors still run
through the existing merge engine.

## Validation and measurement

The final graph, every scheduled Run, ordered RHS input, ordered effective
writes and complete Spine trajectory match the frozen controls in both arms.
There are 3,034 / 663,403 applied resumptions and longest chains of 12 / 29.
The sum-bits audit also passes with forced fingerprint and Sieve collisions.
The Cargo regression passes 19 compiler integration tests and 29 runtime tests
with the existing lifetime and two-Run experiments compiled. Default and
constructor-port-only builds are checked independently.

Tests cover custom merge outputs distinct from their input proposals, old/min/
max behavior, canonical-key repair, retained source replacement and retirement,
borrowed parameter masks, float bit patterns, source-query ports demanded by
consumers, and certificate tampering. These are Cell-versus-Cell checks; they do
not resolve the separate native-egglog equivalence question.

Timing uses ordinary release, one thread, fresh processes, one excluded warm-up
per arm, then five alternating pairs on Apple M3 Max. Both arms enable compact
constructor ports. Only the on arm enables function blocks. Build and offline
training are excluded; complete CLI time includes loading and final export.
The peak RSS high-water is sampled through inference. Diagnostic builds are
never used as performance measurements.

| Pass 3 | Inference ms, off / on | CLI ms, off / on | Peak MiB, off / on |
|---|---:|---:|---:|
| sum-bits | 21.499 / 22.763 | 127.215 / 128.295 | 154.422 / 154.375 |
| Raytrace | 8,431.490 / 8,171.878 | 10,976.657 / 10,749.911 | 1,809.016 / 1,808.031 |

Raytrace's inference median improves by 3.08% and its CLI median by 2.07%, but
only three of five individual inference pairs improve. Sum-bits regresses by
5.88% in inference and 0.85% in CLI. This does not establish a stable general
speedup, so the feature stays disabled by default. The endpoint storage saving
must not be substituted for the measured peak RSS reduction of only 0.98 MiB
on Raytrace.

Raytrace's search / RHS / query-update phase medians are respectively
3,919.990 / 2,180.063 / 1,639.885 ms with the feature off, and
3,792.174 / 2,097.629 / 1,600.198 ms with it on. Matching and publication work
counts are identical. These timing changes do not prove eliminated search or
index work, and phase medians need not sum to the median total.

The next substantial step is a checked state transition containing the function
producer and its scheduled consumers. Its proof must identify which intermediate
function versions are externally observable, preserve ordered merge results and
union-induced key repair, and charge every guard/fallback. Success should be
measured as fewer physical registrations, index changes and wakeups, rather
than a larger count of compact snapshots. That transition is not implemented
by this experiment.

## Cargo commands

Run from the worktree root, using fresh output directories. The standalone
runtime is built separately from the compiler for benchmark and digest runs;
building both packages together can unify their JSON serialization features.

```sh
cargo test --release --locked \
  -p eggfuse-cell-runtime -p eggfuse-cell-compiler \
  --features function-blocks,two-run-survey,origin-demand,entry-retirement,view-ports,storage-metrics

# Emit checked plans when compiling a source; existing frozen artifacts can
# also derive them at load time.
cargo run --release --locked -p eggfuse-cell-compiler \
  --features function-blocks -- compile \
  --source input.egg --output out/compiled

cargo build --release --locked -p eggfuse-cell-runtime \
  --features function-blocks,port-frames,view-ports,storage-metrics \
  --bin eggfuse-cell-runtime --example bench_deferred_incidence

target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/function-pairs \
  out/sum-bits/program.json out/raytrace/program.json 5 --functions

cargo build --release --locked -p eggfuse-cell-runtime \
  --features function-blocks,port-frames,view-ports,storage-metrics,slot-transfer-audit,spine-shapes
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/function-audits \
  out/sum-bits/program.json out/raytrace/program.json 0 --audit-functions

# In-process Cargo bench, optionally checking every Run first.
cargo run --release --locked -p eggfuse-cell-compiler \
  --features function-blocks,port-frames,view-ports -- bench \
  --program out/sum-bits/program.json \
  --reference-program out/sum-bits/program.json \
  --port-frames on --function-blocks on --repeat 5
```
