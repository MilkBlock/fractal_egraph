# Binary fragment tables in the native numeric runtime

This is an opt-in backend for the actual `matrix-numeric` event/commit loop.
It addresses three costs measured by the release benchmark: full JSON heaps,
instance-expanded online layout solving, and CPU marker combination routing.
It uses the existing analyzed Cell proofs and original native function writer;
it does not modify the egglog kernel or claim to accelerate a whole eggcc pass.

## What crosses the process boundary

The original JSON format was useful for inspecting and replaying the common IR.
It was the wrong transport for every live installation. Both live numeric and
live structural matrix installations now send their u32 heap as a raw binary
frame; small plan metadata stays JSON. Offline lowered JSON remains supported.
That change alone still leaves repeated candidate-program construction.

The new table backend avoids constructing that program:

```text
Once, from resolved rule + Cell fragment partitions:
    slot wiring, indexed tuple keys, shared numeric DAG, guards, merge operation
    -> one static device program

Each changed native boundary:
    CPU: helpers, canonicalization, Cell proof admission
    -> binary added fragment rows + retired row IDs
    GPU: update resident tables and indexes
         join changed fragments using exact shared-slot comparisons
         evaluate guard and checked i64 RHS
         reduce pending proposals by canonical target key
    -> binary reduced results, plus small counters
    CPU: optional original-rule oracle, native final min/max merge and set
    -> acknowledge committed batch
```

There is no online solver call, CPU combination enumeration, or per-instance
channel construction on this path. It does not transfer the final native graph.
A fragment can represent several original LHS atoms supplied by a certified Cell;
a regression groups `Single` and its non-root `Get` atom into one fragment table.
The benchmark uses one-atom partitions to keep its previous ablation comparable.

**Cell proof admission and external-fragment extraction still use CPU keyed
reads.** This backend reuses `NumericSession`; it does not inherit the separate
`resident` mode's GPU proof admission. Neither path implements union-find or
congruence closure independently of native egglog.

## GPU combinations without a materialized Cartesian product

One GPU lane starts from a newly added row. A rule-shaped plan chooses another
fragment sharing an already bound slot, then probes its tuple index. Disconnected
parts are visited only when necessary. Hashing locates possible rows; all bound
slots are compared exactly, so collisions do not certify equality.

The first changed **original part ordinal** owns a combination. This ownership
rule is independent of traversal order, so two added rows cannot emit the same
binding twice through different anchors. All compatible matches are retained;
there is no candidate budget or sampling. A truly disconnected join still has
product-sized work. Its matches are streamed to a growing output buffer, not
materialized as a host-side product before execution. If output capacity is
insufficient, only the new uncommitted batch is retried. Earlier completed
records and their acknowledgements survive.

This is an exact GPU indexed join backend, **not a GEMM or Tensor Core speedup**.
The moment/GEMM path remains available as `--matrix-layout paired` or `solver`.
Moving table data to the GPU fixes transport and host routing; it does not by
itself solve the research problem of compiling every join into dense algebra.

## Updates, failures and validation

- Canonical identity codes are typed; numerical fields carry actual i64 limbs.
  They are not float approximations or intern IDs used as numerical values.
- Union/rebuild, function updates and visibility changes retire source versions
  and admit new ones. Retired rows cannot participate in matches or reductions.
- Guard-false bindings produce no effect. Checked arithmetic failure in an
  accepted RHS aborts the batch before any numeric key is written. Valid pending
  results survive and can be committed after the offending input is removed.
- Successful match receipts are acknowledged after native commits. Idle rounds
  do not redo them. Unexpected transport/commit failures poison the session:
  an unconfirmed delta must not be blindly replayed.
- `--gpu-check on` asks original egglog to enumerate the complete active match
  set and independently calculate RHS and final function values. It checks all
  losing proposals too, and verifies GPU proposal reduction before merging with
  the old native value. It runs before any target writes. The default is off.
- Audit mode downloads all active records; normal mode downloads only reduced
  keys and their source references. No match-score matrix is downloaded.

The frontend currently accepts a selected consumer with one `set` to an i64
min/max function, table-bound guards, and supported checked arithmetic. It is
not an arbitrary `.egg` compiler. All consumer atoms must be explicitly covered
by Cell/external partitions. Unknown Cell coverage is not silently fabricated.

Resident row versions and match records are currently retained, including inactive
ones. No garbage collector or general history compression is implemented. Static
index count is at most quadratic in fragment count; retained storage is linear
in row versions times index count, plus successful match records. The exact join
can still have exponential output. Reported device bytes are allocated buffers,
not process RSS. These limitations are visible rather than hidden by truncation.

## Reproduce

Build the release executable and persistent worker using the benchmark helper,
which also builds a clean pristine control (see [benchmark instructions](../bench/README.md)).
Or build only this runtime:

```sh
cargo build --offline --release --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted
xcrun swiftc -O cell-assisted-egglog/matrix.swift -o /tmp/cell-matrix-metal

target/cell-assisted/release/cell-assisted-egglog \
  --mode matrix-numeric --analysis cell-assisted-egglog/fixtures/analyze \
  --consumer-source cell-assisted-egglog/matrix-numeric/consumer.egg \
  --ruleset '' --consumer cost-improve --native single-get \
  --parts '[{"atoms":[0],"cell":true},{"atoms":[1],"cell":false},{"atoms":[2],"cell":false}]' \
  --input cell-assisted-egglog/matrix-numeric/input.egg --rounds 4 \
  --gpu-worker /tmp/cell-matrix-metal --gpu-check on \
  --marker-layout device --matrix-layout tables
```

Live worker protocol is **6**, while offline lowered matrix ABI remains **5**.
Rebuild the worker when updating the Rust executable. Bulk frames use little
endian u32 words and a u32 byte-length header. Small controls/counters remain JSON.
For tables, schema/code/shader metadata is installed once. Each delta is:

```text
[epoch, added_count, retired_count, total_rows,
 added rows: part_id, slot_codes..., numeric_lo_hi...,
 retired_row_ids...]
```

Zero denotes an unbound slot, never a real identity. Output is reduced proposals;
strict checking additionally receives raw match receipts. The 32-bit transport
and device-address ranges are checked; exhaustion is an error, not truncation.

## Measured release result

All **24 variant/case pairs** passed the pristine graph, primitive-value and visibility gates. Each timing is one unchecked fresh process; a separate checked process verifies it. Sizes are 16, 256 and 4096, on Apple M3 Max, four native threads. This is the actual native assistant runtime over controlled Single/Get workloads, not a full eggcc compilation.

| 4096-instance case | Pristine run | Binary paired run | Device tables run | Pristine core | Device tables core |
|---|---:|---:|---:|---:|---:|
| local | 0.0075 s | 5.1165 s | 0.0735 s | 0.6527 s | 0.8196 s |
| sparse-cycle | 0.0169 s | 1.4074 s | 0.1022 s | 1.1860 s | 1.3754 s |

The table path removes the old host routing and program-installation bottlenecks, but **does not yet beat pristine**. It changes both routing algorithm and execution location, so the improvement over paired is not a GPU-only hardware speedup. Core includes static setup and input parsing; the short native run remains far faster. Current CPU Cell admission is about 49 ms (local) / 78 ms (sparse), the largest remaining run phase.

Table uploads total 1,097,760 bytes / 2,106,228 bytes for local/sparse, over two delta batches. Normal downloads total 98,304 bytes / 4,788 bytes. The shader compiles once per session and solver calls are zero. These are bulk protocol bytes; small JSON controls, static shader/config metadata and pipe framing are excluded.

Raw measurement artifacts: `out/table-runtime/final-bench/`; build manifest: `out/table-runtime/final-build/build.json`. Compact results preserve exact binary/input/source hashes. The prior `bench/RESULTS.md` measures the pre-binary JSON implementation and is not silently overwritten.

## Code and evidence

- `src/table_program.rs`: static connected join order, tuple index specifications,
  numerical expression DAG and target-key wiring.
- `tables.metal`: resident indexes, delta traversal, exact guards/RHS, keyed
  proposal reduction and acknowledgements.
- `integer.metal`: checked i64 operations shared with the matrix executor.
- `matrix.swift`: resident allocation, binary transfer and device dispatch.
- `src/table_native.rs`: native row deltas, validation, final merge and commit.
- `src/matrix_service.rs`: binary heap transport for both existing matrix modes.
- `tests/tables.rs`: ten device regressions, including union, late data, two atoms
  supplied by one Cell, high fanout, overflow recovery and corrupted GPU packets.

The full package passed 89 tests with all GPU workers enabled. The benchmark
contract tests reject incorrect values, equivalence, visibility, timing overlaps,
and enabled oracles in timed runs. [RESULTS.md](RESULTS.md) and `results.json`
record fresh release measurements; raw run artifacts remain in the output path
shown there. Timing excludes neither setup nor CPU admission from the core metric.

A subsequent [real raytrace search benchmark](../raytrace-table-bench/README.md)
uses the unchanged 14-rule candidate-extraction family and its production-derived
checkpoint. It reports full native packet phases and a separate exact GPU/native
query comparison. It does not substitute the controlled local/sparse cases for
real eggcc search or claim raw table queries establish Cell coverage.
