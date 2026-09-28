# Why post-saturation packing uses more memory

This is a source-backed allocation diagnosis of the actual local egglog runtime.
No packing semantics were changed. The probe adds a counting System allocator,
phase samples, and a read-only cached-query subset-capacity API. It does not measure
production execution speed. The working tree contains unrelated ongoing kernel work;
`diagnosis.json` records the base commit, dirty state and binary hash.

## 1. The original 17% compression claim used incomplete accounting

`SortedWritesTable::virtual_stats()` computes `dense_equivalent_bytes` from
**physical_len × n_columns × sizeof(Value)**: all physical rows in a converted table,
not just the covered rows. Physical rows may include stale row-ID holes.
`descriptor_bytes` likewise covers the capacity of the entire recipe vector.

Recalculating the original `out/math-pack-rss/pack-full.json`, without changing its data:

| Allocation category | Decimal MB |
|---|---:|
| Recipe/descriptor vector | 20.447 |
| Uncovered direct-row values | 15.402 |
| Local headers, instance references, Box pointer arrays | 19.451 |
| Shared binding payload | 1.660 |
| Other counted template/header/tail payload | 0.003 |
| **Known total** | **56.963** |
| Dense equivalent of those physical tables | **24.631** |

Thus even the known representation accounting is **2.31× dense**, not 17% smaller.
Indexes, tombstone hash allocations, allocator overhead, pools, and spare dense capacity
are outside this accounting, so this is not a complete retained-heap or RSS ratio.
The richer library has nearly the same known cost: 56.680 MB against 24.617 MB.

## 2. A tiny packing request converts entire native tables

In `table/virtual_rows.rs`, `pack_rows` calls `encode_virtual_base`, which traverses
`0..physical_len()`. Covered rows get a template recipe. **Every uncovered live row
gets its own `Box<[Value]>` plus a recipe**, replacing a compact contiguous row buffer.
Stale row IDs still occupy descriptor positions and enter the tombstone set.

Isolated round-1 pattern 2 on the full 11-round Math graph:

- Only **3 matched instances**; only **19 virtual rows** (8 Add and 11 Mul).
- Nevertheless, **986,799 uncovered live rows** become direct boxed rows in the two
  converted tables (641,735 Add + 345,064 Mul).
- The virtual bases also retain 667,977 stale row-ID holes/tombstones in this run.
- Rust requested live allocations increase **38.99 MiB during the pack call**.
- Final Math graph remains 1,047,896 enodes / 443,832 eclasses.

The original dense buffer is replaced with an empty `Rows` buffer; the code does **not**
simply retain a complete second dense graph alongside the virtual base. The new per-row
representation, holes and pointer arrays are the problem. Existing native hashcons,
row IDs, timestamps and indexes are intentionally preserved, so they provide no memory
saving here either.

On the complete round-1 library, single-thread controls finish with **196.63 MiB live
Rust allocations for match-only versus 229.03 MiB for pack**. Summing just the live
allocation deltas across each pack call independently gives **+32.40 MiB**. This isolates
an actual storage penalty from the query/RSS retention problem.

## 3. Large cached queries retain duplicated concrete candidate sets

The first fragment in the richer library has **115 atoms, 37 projected slots** and
only **36 final matches**. Its result tuples cannot explain GiB of memory.

The new read-only `fact_query_subset_bytes` measurement finds:

| Cached-plan allocation | Bytes |
|---|---:|
| Header sparse row-ID vector capacities | 221,232,568 |
| Atom sparse row-ID vector capacities | 221,232,568 |
| **Total** | **442,465,136 (421.97 MiB)** |

Compilation increases live requested allocation by 454,430,981 bytes in the isolated
single-thread run. The two candidate-set copies account for **97.37% of that increase**.
Freeing the query after execution releases 442,686,335 live bytes, corroborating ownership.

The source mechanism is concrete: `ProcessedConstraints.subset` belongs to each atom;
planning clones it into `JoinHeader.subset`; a sparse `Subset` owns a
`Pooled<SortedOffsetVector>`, whose backing storage is a `Vec<RowId>`. Cached-plan cloning
also clones header subsets. This is cached **data**, not merely a compact abstract query
plan. Repeated uses of the same native table therefore pay for candidate row sets repeatedly.
Execution has additional temporary plans/subsets/indexes; the isolated process reaches
2.04 GiB of peak requested Rust allocations. This probe does not claim a complete
allocation-site breakdown of that transient peak.

## 4. High post-query RSS is not all live graph storage

For that isolated 115-atom query, after its result and query are freed:

- Math graph unchanged; no virtual rows were created.
- Live requested Rust allocations: **204.19 MiB**.
- Current RSS: **2,038.75 MiB**.
- After library drop and allocator pressure relief: live **196.98 MiB**, RSS **1,565.22 MiB**.
- After dropping the egraph and another relief: live **43.76 MiB**, RSS **1,117.22 MiB**.

The remaining RSS cannot be interpreted as live RipSatCell storage. It includes allocator/
VM retained resident memory. Some live memory also survives engine destruction because
core-relations has thread-local `ManuallyDrop<PoolSet>` caches. The probe does not assign
all remaining live bytes to pools, and pressure relief is not guaranteed to return every
resident page. Rust live counters exclude non-Rust allocations; RSS includes them.

Default-thread full-r6 diagnostics hit the external 4 GiB guard during pattern 2 in both
match and pack modes. Those aborted runs are not presented as completed r6 measurements.
Isolating pattern 0 finishes and suffices to establish the query duplication. Allocation
instrumentation and thread count affect RSS; these new values are not replacements for
the original uninstrumented report's historical measurements.

## 5. Why the previous RipSatCell speedup does not resolve this report

This report runs `native_storage::pack_library` (`src/reuse_virtual.rs`): rediscover whole
fragments in the finished graph, collect all matches, and call single-template packing.
It does **not** run the newer cell-machine path, which obtains known bindings from
applications and calls `pack_cell_wave`. Avoiding rediscovery can remove the large-query
problem, but both paths still use the same native virtual-base representation. The
whole-table boxing problem remains relevant to both. Parameter symmetry alone does not
repair either storage layout or duplicated query subsets.

## Fix order suggested by the evidence

1. Separate reusable query structure from concrete candidate row sets; share immutable
   table/version/constraint subsets rather than copying them per atom/header. Refresh on
   changes. Avoid discovering an entire saturated body when certified application bindings
   are already available.
2. Keep uncovered rows in contiguous native storage. A sparse packing request must not
   force unrelated rows into per-row boxes. Charge the full representation cost, including
   descriptors and indexes, before claiming any net compression.
3. Re-run equal-graph live-allocation and RSS comparisons. Only after this should symmetry
   normalization be credited with storage gains.

## Reproduce

```sh
cargo build --offline --release --example virtual_memory_audit
RAYON_NUM_THREADS=1 ./target/release/examples/virtual_memory_audit \
  egglog/tests/math-microbenchmark.egg out/math-lib-full/library.json match
RAYON_NUM_THREADS=1 ./target/release/examples/virtual_memory_audit \
  egglog/tests/math-microbenchmark.egg out/math-lib-full/library.json pack
MEMORY_AUDIT_PATTERN=2 RAYON_NUM_THREADS=1 ./target/release/examples/virtual_memory_audit \
  egglog/tests/math-microbenchmark.egg out/math-lib-full/library.json pack
MEMORY_AUDIT_PATTERN=0 RAYON_NUM_THREADS=1 ./target/release/examples/virtual_memory_audit \
  egglog/tests/math-microbenchmark.egg out/math-lib-r6/library.json match
```

`MEMORY_AUDIT_PATTERN` selects one diagnostic query, not a runtime candidate heuristic.
The underlying library generation commands remain in `README.md`. Detailed samples and
historical accounting are in `diagnosis.json`; large raw outputs remain under `out/`.
