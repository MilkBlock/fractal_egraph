# cell-assisted-egglog

A local research fork that uses **completed analyze Cells to replace parts of a
consumer query**, then runs the original egglog RHS. The native kernel is real;
this is not a simulation. It is not yet a complete eggcc planner replacement.
Work lives on `codex/cell-assisted-egglog`; the pristine `egglog-baseline` tag is
unchanged. Initial fork base: `9ea85e47824fcef4e4336fe2f2e3160d37169f73`.

## Resident GPU path

`--mode resident` moves Cell instance maintenance and residual matching onto
Metal. There is **no runtime diagnostic TraceSession and no CPU Cell registry**.

```text
startup (CPU, once)
  analyze history + completed RipSat states + selected consumer/partitions
    → typed template program, forward reads, projections, join interfaces
    → upload static program

native execution (CPU)                     persistent device state (GPU)
  original helper rules                     tracked fact rows + canonical IDs
  committed insert/update/delete/subsume ───→ dirty Cell dependencies
  committed union/rebuild redirects     ───→ invalidate / revalidate proofs
  compact producer bindings             ───→ intern pending Cell instances
                                             ↓ exact keyed validation
                                            valid Cell instances
                                             ↓ projection + alias checks
                                            fragment bindings + tuple indexes
                                             ↓ delta matching
  original consumer RHS ←────────────────── new complete bindings only
  native insert / union / rebuild             (no full score matrix download)
```

A producer event is a **possible occurrence**, not proof of a mutation or of a
closed Cell. The device admits it only after all required visible rows, literals
and equalities agree with committed native facts. Pending instances watch missing
keys. Union or row changes wake dependent instances; deletion/subsumption can
revoke admission. The native engine remains authoritative for union and rebuild.
The device receives canonical redirects; it does not independently implement
congruence closure.

Reverse watches, Cell values, fragment projection, duplicate elimination, tuple
indexes and binding assembly reside on the GPU. The CPU transports compact event
packets, schedules Metal commands, and executes accepted RHS bindings. Thus this
removes **CPU semantic Cell maintenance**, not all host involvement. Swift is a
persistent Metal worker process; transport and host dispatch still have a cost.

The consumer is omitted from the native helper run. Its original RHS is compiled
once against a one-relation binding worklist. No second full-LHS consumer searches
in parallel. Every returned binding uses the same native typed values; the
engine is frozen during device matching and batches carry checked epoch IDs.

## What changed in the kernel

- Opt-in, per-table committed-key journals coalesce actual mutations to their
  final row/visibility. They include serial and parallel writes, removals and
  rebuild changes. Proposed writes and duplicate no-ops are not new facts.
- Union-find exports new displacements since a cursor, with their current roots.
- Native producer queries can add a callback containing only the requested
  canonical binding fields. The original seminaive / `:naive` mode is retained.
- Journals are independent across EGraph clones. A subscription is exclusive to
  one engine scope; it is not a push/pop or multi-subscriber protocol.

With journals disabled, the normal execution path retains its original behavior;
there is an extra opt-in branch at commit sites. No zero-overhead claim is made.
Existing tables and native hashcons remain explicit. This is **not** a virtual
fact storage or RSS-compression result.

## Matching and resource behavior

The new resident matcher uses **exact integer tuple indexes**, not MPS GEMM.
Each changed fragment starts a baked query plan. Earlier parts must be unchanged;
this partitions matches by their first changed part and avoids duplicate binding
emission across anchors. Shared-variable interfaces are compiled once per query,
not cached per historical application path. Native rule predicates unsupported by
the compiler are rejected, not guessed.

Only dirty Cells perform proof lookups, although the resolver still scans their
dirty flags. When fragment membership changes, projection and index construction
still scan resident references. An idle round submits no GPU work. Output/watch
buffers grow and retry without candidate truncation. Moving a buffer preserves
index links; only a change in hash bucket count requires rehashing.

The current implementation retains old instance keys, watchers and fragment
bindings. Inactive entries are filtered but not garbage-collected. The reported
`resident_bytes` is the allocated mutable GPU heap, **not process RSS**; it excludes
the static program, shader/compiler, command buffers and native engine.

A finite baked library does not cover every future instance. In particular,
helper closure may establish a Cell only in a later driver round. Repeated
unchanged bindings are suppressed; this is not a promise to reproduce arbitrary
native schedules, destructive delete/reinsert histories, or every logical match.

## Controls and limitations

| Mode | Cell maintenance | Residual matching |
|---|---|---|
| `off` | none | native egglog query, no tracing |
| `cpu` | CPU, dependency trace + keyed reads | CPU Cartesian compatibility |
| `gpu` | same CPU maintenance | Metal/MPS Cartesian compatibility |
| `resident` | GPU, compact native events | GPU indexed delta matching |

The legacy `gpu` path encodes each shared 32-bit ID using 32 signed bits. The dot
product reaches `32 * shared_slots` exactly iff all IDs agree. It returns tiled
score matrices for CPU binding assembly. The new path changes **both maintenance
and matching algorithm**, so their timing difference does not isolate residency.
The old report is preserved in `results.json`.

All paths currently support positive constructor/relation consumer queries and
constructor/relation/union actions. Primitive filters, custom function lookups,
globals and containers are outside the resident consumer compiler. The compact
native journal itself is tested with function updates/custom merges, but that
alone does not add those operations to the GPU query compiler.

Templates come from actual analyze output, including non-root projections and
conditional aliases. The loader currently accepts single-application triggers
with direct source-variable interfaces and identity catalog adapters. Concrete
literals are retained: baking index 3 does not authorize index 7. Cell projections
and atom partitions must cover every atom of the selected consumer exactly once.
Consumer selection and partitioning remain explicit. This is **not yet** a
Free Join mid-query handoff, arbitrary graft-state execution, or automatic
construction of new Cell families on the GPU.

The native helpers and GPU consumer run in explicit driver rounds. Correctness
controls compare final typed graphs and complete binding sets; they do not assert
one driver round equals one original ruleset iteration. `--audit true` queries the
full native LHS separately, rejects duplicate/invalid emissions, and counts
current bindings not yet observed. The oracle never supplies candidates. Audits
and graph dumps are excluded from reported execution timing.

## Reproduce offline

From this worktree root, on macOS with Metal and the Xcode toolchain:

```sh
CARGO_PROFILE_DEV_DEBUG=0 cargo build --offline --bin egg_layout --target-dir target/analyzer
python3 cell-assisted-egglog/validate.py --output out/cell-resident-new --if-instances 300

CELL_RESIDENT_WORKER="$PWD/out/cell-resident-new/metal-resident" \
CELL_METAL_WORKER="$PWD/out/cell-resident-new/metal-residual" \
RAYON_NUM_THREADS=4 cargo test --offline \
  --manifest-path cell-assisted-egglog/Cargo.toml --target-dir target/cell-assisted
```

The script freshly analyzes three small programs, builds both workers and the
release executable, then runs each control once with the same held-out input.
The optional sparse If case reuses the original min/max/Single rules: 10% of
inputs enable min/max, 90% are near misses. Final graph equality includes all
six sorts; an independent check validates every If result and every near miss.
Separate audit runs compare bindings. `--cpu-only` omits both GPU paths. Output
directories must be new. GPU tests require their worker environment variables;
without them those integration tests return without exercising Metal.

```sh
target/cell-assisted/release/cell-assisted-egglog \
  --analysis out/cell-resident-new/eggcc-getters/analyze \
  --consumer route-get --native single-get \
  --parts '[{"atoms":[0],"cell":true},{"atoms":[1],"cell":false}]' \
  --input out/cell-resident-new/eggcc-getters/input.egg --rounds 3 \
  --mode resident --gpu-worker out/cell-resident-new/metal-resident
```

`off` is the same fork without assistance, **not pristine upstream**. Setup,
input and run times are reported separately. These controls include native
helpers, event capture, GPU round trips, original RHS and rebuild. Startup shader
compilation is real cost even though it is not part of `run_seconds`. Neither a
faster device kernel nor fewer emitted matches implies end-to-end acceleration.

## Code map

- `../egglog/src/cell_events.rs`: compact producer callbacks and committed deltas.
- `../egglog/src/cell_assist.rs`: typed query schema, original RHS sink, audit.
- `src/lib.rs`: analyze import and symbolic projections; legacy CPU runtime.
- `src/resident.rs`: one-time program compilation and event/RHS transport.
- `resident.metal`: persistent Cells, exact proof validation and delta matching.
- `resident.swift`: Metal buffers, commands, lossless allocation/retry, binary I/O.
- `src/matrix.rs`, `gpu.swift`: legacy CPU/MPS compatibility controls.
- `validate.py`, `tests/`: full-graph, native-binding and lifecycle regressions.

## Resident verification, 2026-10-18

`resident-results.json` records the latest single release executions on an Apple
M3 Max (one native worker thread). All four modes produce the same final graphs.
Separate lifecycle tests use four native worker threads.

| Input | Native fork | Auxiliary CPU | Legacy GPU | Resident GPU |
|---|---:|---:|---:|---:|
| getter rules | 0.124 ms | 0.172 ms | 24.836 ms | 4.162 ms |
| three If inputs | 0.466 ms | 0.972 ms | 34.192 ms | 8.865 ms |
| two neighboring Cells | 0.066 ms | 0.195 ms | 15.712 ms | 8.394 ms |
| 300 sparse If inputs | 9.369 ms | 43.323 ms | 262.485 ms | 37.537 ms |

The 300-If case has **4,868 constructor nodes / 4,238 e-classes** across six sorts;
Expr alone has 3,960 / 3,330. Every mode produces 15 min, 15 max and 270 near misses
without a result. The resident GPU is about 7 times faster than the legacy GPU
path here, but remains about 4 times slower than native. It is not an eggcc speedup.

For that case, the resident path maintains 630 active Cell instances, mirrors
4,563 required rows, and returns 15 consumer bindings (1,080 binding bytes). It
uploads 212,428 event-packet bytes and allocates a 13,523,716-byte mutable GPU heap.
No complete score matrices or fragment tables return to Rust. These byte counters
exclude startup and small JSON control/metrics frames. Native helpers still
perform 615 matches. Producer observations and dispatched bindings are **not**
committed mutation counts.

CPU semantic Cell validations, CPU Cell point reads and diagnostic trace events
are absent on this path. The GPU performs 1,230 validations including allocation
retries, and 2,880 exact fact lookups. There are two device batches and one idle
round. GPU command time is 18.23 ms; host/device round-trip time is 30.71 ms,
native helpers 5.36 ms, event packaging 1.19 ms, RHS 0.25 ms. Device command time
is contained in round-trip time, not added to it. Setup costs another 122.80 ms
for this run (including 118.34 ms resident compilation/worker setup); input parsing/import costs another 149.65 ms. These startup costs are excluded
from the execution table; full phase timings are in the JSON.

All 27 tests passed with both Metal workers enabled, including parallel native
commits, update/merge/rebuild feeds, `:naive` preservation, late union, subsumption,
late/reinserted facts, overlap after union, and lossless buffer growth. Separate
native binding audits report `[0,0,0]` for the getter and both If workloads. The
neighboring-Cell case reports **`[2,0,0,0]`**: two first-round native matches wait
for the helper closure to complete. This coverage gap is retained in the report.

## Larger, production-derived reference

The [raytrace Pass-3 checkpoint benchmark](snapshot-bench/README.md) exercises
14 unmodified candidate-extraction rules on 421K initial table rows. It takes
8.406 seconds, with independently measured sparse keyed results and exact native
replay checks. This is the new substantial reference; the tiny If cases above
remain correctness controls. The resident compiler accepts only 3/14 rule bodies
syntactically, so a complete GPU comparison is explicitly unavailable.
