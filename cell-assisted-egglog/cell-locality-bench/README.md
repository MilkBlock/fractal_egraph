# Classifying partial queries by usable Cell context

This is an opt-in extension of the actual shared GPU/native runtime. It grades
**partial bindings**, not whole rules and not materialized five-way Cartesian
products. Unknown and ambiguous branches still execute. Original RHS actions,
union/rebuild, query watermarks and the ruleset commit boundary are preserved.

The frozen library is the existing analyzed `Single/Get` Cell from eggcc's tuple
helper fragment. A ready context guarantees `s = Single(x)` and `Get(s,0) = x`.
It is a **positive, versioned Cell interface certificate**, not an assertion
that an arbitrary whole Cell instance has been fully identified or that every
consumer's complete LHS is internal. This does not install the separate CPU
whole-Cell A → B → C executor on the GPU.

## A concrete five-atom consumer

```egg
(rule ((Boundary s n) (= s (Single x)) (= x (Get s 0))
       (Offer s k) (Allowed k) (>= n 0))
      ((Seen x n)))
```

Starting from a new `Boundary(s,n)` binds `s,n`. A keyed reverse lookup can locate
`Single(x) = s`, giving the exact context handle. The ready Cell can supply the
remaining `Get` binding. `Offer`, `Allowed` and the numeric guard still need to
hold. Two covered atoms do **not** mean two lookups were eliminated: the locator
itself reads one; this route eliminates one internal lookup.

A delta in `Allowed(k)` can require an extra bridge through `Offer(s,k)` before
it reaches the Cell. The compiler now prefers entering an anchored witnessed
fragment instead of unrelated branching. Decomposition follows uniquely
resolved atoms until it reaches a Cell, has no useful route, lacks a bound
lookup key, or encounters a branch. The query length bounds this process; there
is no depth/sample budget. Finding a second compatible row is a proof of
ambiguity, not permission to keep only the first.

## Execution and invariants

1. Small GPU threads bind one original semi-naive delta atom and reject wrong
   table/constant/alias combinations before allocating deep join state.
2. Classify the partial binding:
   - **Ready:** one located current Cell interface has a valid certificate and
     compatible binding. Enqueue its context ID, revision and covered-atom count.
   - **Suspected:** a Cell route exists but the witness is waiting/uncertified, or
     its location branches. Do not infer a complete match or pick one instance.
   - **Unmapped:** decomposition has not reached a usable Cell route. This does
     not mean that no Cell could be used later.
3. Carry every uniquely located prefix row's event ID into the work item. Deep
   execution reuses those rows instead of repeating the classification lookups.
4. If exact Cell/delta ownership already delegates a seed to a ready-port query,
   remove that seed from deep matching. A missing required next atom or another
   earlier delta owner also proves that this seed cannot produce a new match in
   this snapshot. These reasons are counted separately from priority queues.
5. Run ready ports, then Ready, Suspected, and Unmapped queues on the **same
   immutable snapshot**. Finish all required work before committing any RHS.
   Tier reordering does not create a new rule schedule or advance a watermark
   after only the high-priority subset.
6. Queue capacity grows to the observed count and classification is retried on
   overflow. It never truncates candidates. Count identities are checked:
   valid seeds = queued + missing-next + port-delegated + earlier-owner.

The interface ID is an allocated slot, found through the native row's observed
projection list. Its revision belongs to this resident interface instance, not
a whole RipSat state ID. IDs can be recycled; revisions are preserved across
reuse. See [sparse storage](../../experiments/sparse_cell_storage/README.md).
Native events invalidate/revalidate certificates before classification.
Queue hints never persist across commit/rebuild epochs. New union alternatives,
subsumed rows, row recycling and new specializations retain normal matching.

## Reuse within one Cell

`PortLocality::SharedContext` assigns a GPU workgroup to a ready context and
loads its interface frame once into threadgroup memory. Different consumers
retain independent residual queries. `SharedJoins` additionally shares exactly
alpha-equivalent residual programs. Canonical slot maps preserve each consumer's
binding, guards and RHS; numerical guards are never baked away. Consumers are
only joined in the same batch group when their native watermark and port cursor
agree. Grouping costs O(number of active rules) host work per batch; it does not
read or identify concrete Cell instances on the CPU.

Residual query equivalence is ordinary query sharing enabled by the common Cell
coordinates. These results do not establish that generic multi-query optimization
without Cells could never obtain the same benefit.

## Controls and metrics

`Options` exposes independent controls, defaulting to the existing path:

```rust
Options {
    analysis: Some(catalog),
    profile: true,
    port_locality: PortLocality::SharedJoins,
    query_routing: QueryRouting::CellPriority,
    ..Options::default()
}
```

Routing controls are `Direct`, `Compact` (no Cell classification), and
`CellPriority`. This separates generic work compaction from Cell-based routing.
Locality controls are `Independent`, `SharedContext`, and `SharedJoins`.

Report each ruleset/workload separately:

- Ready/Suspected/Unmapped **seed** fractions, with excluded reasons separate.
- Actual reuse rate in each tier, including late Cell reuse in low-priority work.
- Covered atoms / query atoms, alongside actual skipped reads; these are different.
- Fraction of seeds producing a complete binding. This is not committed mutation
  yield and is not the same as successful Cell reuse.
- Locator index probes, candidate rows, carried unique prefixes, ambiguities,
  classification retries, and additional device buffer bytes.
- Shared context loads, residual query executions and index probes.

Counter loads/words describe logical requested reads, **not measured DRAM traffic
or cache misses**. Index probes and candidate rows also have different meanings.
Profiling atomics are disabled in the separate timing run. Classification may
cost more than it saves; modes are opt-in. Current ready-seed queues are grouped
by priority, not additionally sorted by context ID. The ready-port workgroup
path already shares one context across consumers.

## Reproduce

```sh
mkdir -p out/cell-locality-worker
xcrun swiftc -O cell-assisted-egglog/matrix.swift -o out/cell-locality-worker/matrix
cargo build --offline --release --example cell_locality_bench --target-dir target/analyzer

# WORKER CATALOG NEW_OUTPUT_DIR INSTANCES CONSUMERS MODE FANOUT [mixed]
RAYON_NUM_THREADS=1 target/analyzer/release/examples/cell_locality_bench \
  out/cell-locality-worker/matrix cell-assisted-egglog/fixtures/analyze \
  out/locality-profile 1024 8 profile 4 mixed

RAYON_NUM_THREADS=1 target/analyzer/release/examples/cell_locality_bench \
  out/cell-locality-worker/matrix cell-assisted-egglog/fixtures/analyze \
  out/locality-time 8192 8 time 4 mixed

RAYON_NUM_THREADS=1 target/analyzer/release/examples/cell_locality_bench \
  out/cell-locality-worker/matrix cell-assisted-egglog/fixtures/analyze \
  out/locality-check 256 8 check 4 mixed

CELL_MATRIX_WORKER="$PWD/out/cell-locality-worker/matrix" RAYON_NUM_THREADS=4 \
  cargo test --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted --test query_routing --test port_locality \
  --test cell_prefix --test cell_transitions --test table_live
```

Build both Rust and Swift: the resident family buffer layout is now **5** (the
outer live protocol remains 10). Old workers are rejected before query execution.

The generator uses held-out literal identities and the frozen catalog. Its
`mixed` boundary creates known, missing-witness and ambiguous situations via
subsume/union, plus new `Allowed` rows that require a multi-atom bridge. This is
a controlled **synthetic** multi-consumer workload on a real analyzed fragment,
not a whole eggcc benchmark or an estimate of eggcc's Bake quality.

All seven variants compare complete original tables, values, equivalence and
visibility with native execution at both boundaries. Check mode additionally
compares each complete incremental frontier before RHS execution. Timings cover
`Runtime::run`: preparation, transfer, GPU work, decoding and native RHS/merge/
rebuild. Setup/shader compilation and externally supplied fact input are separate;
`external.input_seconds` records the latter. One execution per variant, no average.

See [RESULTS.md](RESULTS.md) for evidence and the limits of the timing result.
