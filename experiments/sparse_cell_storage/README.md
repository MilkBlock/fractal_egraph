# Allocate observed Cell interfaces, not rows × plans

This changes the actual `table_live::Runtime` Metal backend. The Bake catalog,
query compiler's 24 rich-library interface plans, rule guards, native commit
boundaries and fallback queries are unchanged. Results and a preserved dense
control are in [RESULTS.md](RESULTS.md); the controlled workloads are the same
[Bake quality ablation](../bake_quality/README.md), not a full eggcc run.

## Storage and event ordering

Previously `cell_id = row_slot * number_of_plans + plan_id`. Every native row
reserved all interface frames; dependency capacity further multiplied this by
the maximum prefix length. The replacement is:

```text
row_slot -> head -> (observed anchor, projection, frame, next) -> ...
                      |                 |
                      |                 +-> current reached dependency keys
                      +-> revision-qualified ready port, when certified
```

1. Wake old subscribers using old and new native keys, then apply committed row
   changes. Traverse only the changed rows' allocated instances; retire anchors
   that no longer match. Static per-table plan lists identify possible new anchors.
   Constants and repeated-variable equalities are checked before allocation.
2. GPU counts actual missing instances. The host sizes a pool from that count;
   GPU allocates recycled slots first. A one-word row directory replaces the
   Cartesian record array. Retired slots retain watch/port revisions to prevent
   stale certificates attaching to new instances.
3. Evaluate each affected interface once. Save the witness row IDs and generations,
   reached-prefix length and Ready/Waiting/Uncertified state. Count exactly the
   reached keys, including the first **missing** dependency, and Ready ports.
4. Size dependency/port storage from these counts and live old versions. When a
   log fills or anchors retire, count/compact surviving versions first. Publish
   keys by rebinding the saved witnesses, **without repeating index searches**.
   Reset port cursors on compaction; native timestamps still enforce per-rule
   seminaive ownership, including delayed consumers.
5. Run the same query tiers on the frozen native snapshot. New facts, subsume,
   union and row reuse can invalidate interfaces; unmatched/uncertified cases
   keep their normal query path. All required queries finish before native RHS.

Instance detection, subscriptions and state updates stay on the GPU. The host
reads aggregate allocation counts, allocates buffers and dispatches kernels; it
does not walk Cell bindings. These host/device barriers cost time. The initial
empty subscription directory is explicitly initialized before any wake.

With `N` native row slots, `I_peak` observed peak interface count, frame width
`F`, and currently retained dependency/port logs `W`/`P`, auxiliary dynamic
storage is `O(N + I_peak*F + W + P)`, instead of `O(N*plans*F)` frames plus
worst-case dependency reservations. `F` still uses the compiled maximum frame
shape. Power-of-two capacity rounding, reusable free slots and stale log entries
until compaction remain. This is not a minimal-byte encoding or a shrinking
allocator after a large historical peak. Native rows, indexes and result queues
are also still stored. Temporary old/new buffers during growth/compaction are
excluded from the persistent-byte metric.

Lookup walks the short observed-instance list of one row. Very many projections
on the **same actual anchor** can still cost time and memory. We have removed
unobserved row/plan products, not promised constant cost for arbitrary catalogs.

## Files

- `cell-assisted-egglog/family_registry.metal`: discovery, retirement, pooling.
- `family_events.metal`: evaluate/count then exact dependency publication and GC.
- `family.metal`, `family_route.metal`, `family_ports.metal`, `family_cells.metal`:
  sparse lookup in queries, ports, routing and cache-only mode.
- `matrix.swift`: aggregate sizing and ordered GPU stages; diagnostics.
- `src/family_program.rs`: per-table anchor dispatch, family layout version 6.
- `tests/sparse_cells.rs`: unrelated rows, anchor constants, late dependencies,
  separate- and same-batch retirement/reuse, stateful RHS, cache-only mode.

## Reproduce

From the repository root, use **new** output directories:

```sh
xcrun swiftc -O cell-assisted-egglog/matrix.swift -o out/cell-locality-worker/matrix
cargo build --offline --release --example cell_bake_quality --target-dir target/analyzer
RAYON_NUM_THREADS=1 target/analyzer/release/examples/cell_bake_quality \
  out/cell-locality-worker/matrix out/sparse-bake-check 32 check
RAYON_NUM_THREADS=1 target/analyzer/release/examples/cell_bake_quality \
  out/cell-locality-worker/matrix out/sparse-bake-profile 128 profile
RAYON_NUM_THREADS=1 target/analyzer/release/examples/cell_bake_quality \
  out/cell-locality-worker/matrix out/sparse-bake-time 512 time

CELL_MATRIX_WORKER="$PWD/out/cell-locality-worker/matrix" RAYON_NUM_THREADS=4 \
cargo test --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted --test sparse_cells --test cell_prefix \
  --test cell_transitions --test query_routing --test port_locality \
  --test table_live -- --test-threads=1

python3 experiments/sparse_cell_storage/compare.py
```

The dense control used binaries frozen from `f120fba` before this change. To
rebuild that control, check out that revision separately, compile **both** its
Swift worker and Rust example with the commands above, and run its example with
its worker and `512 time`. Do not mix dense layout 5 and sparse layout 6.
No build artifacts are tracked; `results/manifest.json` records source/workload
hashes. Source, held-out input and perturbation files are byte-identical between
the two timed runs. Reports retain startup and all inference phases, not just
the faster phases. No timings are averaged or silently discarded.
