# Native whole-RipSatCell storage

This changes the actual egglog table representation, not just an exported graph.
A `NativeCellBatch` retains a complete shared body (operator table IDs and slot
patterns), its instance bindings, and an ownership/metadata variant. Operator tables
hold projections of that batch. A covered fact's physical position is derived from
projection start + instance ordinal; it has no standalone recipe or native hash entry.
Overlapping cells keep one physical owner per fact. The full body remains available
while ownership identifies the facts actually stored by this batch.

Existing eclass IDs are preserved. Bindings are encoded losslessly using constant,
affine, or packed-delta columns when their total allocated representation is smaller
than the flat array; otherwise the flat array is kept. This is integer-ID compression,
not a new algebraic equivalence or a claim that internal identities can be discarded.
Lookups use shared per-binding-column orders, or direct arithmetic for uniform/affine
columns, followed by exact key checks. No probabilistic hash or candidate budget is used.

Compact tables release native hash entries for covered facts, persistent column/tuple
indexes, and populated rebuild indexes. Uncovered facts retain ordinary hashcons keys.
Union rebuilds find affected cells through bindings, and ordinary inserts/updates/deletes
use a native delta overlay. Query iteration still exposes the same logical facts.
Compaction may renumber physical row IDs and increments table generation; eclass IDs,
row metadata, native merge behavior and committed provenance stay intact.

Heavy invalidation can invoke the existing stale-row cleanup policy and materialize a
table. Explicit materialization is also supported. Both are reported by
`materialized_fallbacks`; this is not a promise of permanent encapsulation under arbitrary
mutations. Future repacking can restore compact storage from known complete instances.

## API and execution

- `EGraph::compact_cell_facts`: validates all supplied committed facts and compacts a
  cross-table batch. It never creates inferred logical facts.
- `EGraph::compact_cell_storage`: separate body/binding/index/projection accounting.
- `native_storage::compact_library`: post-saturation experiment using the existing
  library discovery path; this still pays for discovering complete fragment bindings.
- `EGG_LAYOUT_COMPACT_CELLS=1`: use known instance bindings from the adaptive runtime's
  existing packing wave. This avoids library-fragment rediscovery but does not change
  which baked recipe that runtime executes.

No default native rule semantics or pristine source is replaced. The compact path is
explicitly selectable for comparison with the previous row-virtual representation.

## Reproduce

```sh
cargo build --offline --release --example post_pack_bench --example adaptive_math \
  --example virtual_memory_audit
python3 experiments/compact_cells/bench.py --output out/cell-recheck --runs 1
```

The native benchmark runs the unchanged 11-round Math program and verifies full graph
correspondence against the recorded pristine binary in separate runs. It measures native,
legacy row packing, and whole-cell packing, including library loading and discovery.
Offline Bake and compilation are excluded. A post-saturation compaction cannot erase
memory already needed to build the original graph; peak RSS and retained allocations
must be reported separately.

For the frozen adaptive runtime (same recipe set in both controls):

```sh
RAYON_NUM_THREADS=1 EGG_LAYOUT_RUNTIME_SUMMARY=1 EGG_LAYOUT_COMPACT_CELLS=1 \
  cargo run --offline --release --example adaptive_math -- frozen \
  experiments/math_mini3/source.egg out/cell-adaptive \
  experiments/math_mini3/adaptive_runtime/bake_lib_round2.json
```

Its graph may differ from ordinary egglog after the same number of rounds because baked
closures advance reasoning. It is not an equal-work pristine speed comparison.

## Verified result

The unchanged 11-round graph is exactly equal across pristine, current-native, row
packing and cell packing: **1,047,896 Math enodes / 443,832 eclasses**. The compact form
owns **277,803 existing facts**. No logical expansion or materialization fallback occurs
in this post-saturation run.

The native hash directories no longer contain those covered facts. For example:

| Table | Facts in cells | Facts left in native hash |
|---|---:|---:|
| Add | 188,725 | 453,018 |
| Mul | 82,880 | 262,195 |
| Diff | 4,881 | 8,623 |

The instrumented live-allocation experiment observes **−82.03 MiB** across the storage
replacement calls, versus about −4.47 MiB for the previous row-virtual representation.
This is real live-allocation reduction, not just a smaller descriptor counter. It
includes releasing populated native indexes; it is not all binding compression.

Deduplicated compact-state accounting in that run: 921,908 binding-storage bytes,
759,712 binding-index bytes, 369,560 cell-header bytes, and 2,272 shared code bytes.
Table projections are reported separately. These totals exclude uncovered ordinary
facts, union-find, global interners, transient query work and allocator retention.
They must not be presented as the memory occupied by the whole egraph.

### Whole-process boundary

One-pass, System allocator, default native parallelism; verification exports are separate:

| Mode | Through execution/packing | Peak RSS |
|---|---:|---:|
| Pristine | 0.278 s | 474.2 MiB |
| Current native | 0.271 s | 354.0 MiB |
| Previous row packing | 1.596 s | 548.8 MiB |
| Whole-cell packing | 1.631 s | 567.7 MiB |

**Peak-RSS acceptance against pristine still fails.** This experiment first builds the
ordinary graph and discovers library fragments, then compacts it. The reduced final
storage cannot erase that earlier allocation peak. This is not a throughput win over
bare egglog; measurements are a single screening pass, not a stable speed claim.

The known-binding integration is real: with `EGG_LAYOUT_COMPACT_CELLS=1`, the frozen
adaptive runner uses compact batches at each packing boundary. Its six-round graph
matches the row-storage control exactly (2,167 enodes / 748 eclasses). The screening
run takes 25.96 ms versus 15.67 ms for row storage, so this mode remains explicit.
The online script asserts the compact hook is active rather than trusting an environment
variable. Those frozen recipes advance inference differently from ordinary native rules.

Native tests cover continued unchanged Math rules across compact storage, external
joins, union collisions, subsume, function merges/deletion, overlapping ownership,
repacking, snapshots, producer certificates after physical row renumbering, and explicit
materialization. Column-code tests cover constant/positive and negative affine/packed
IDs and cross-word bit boundaries. Queries may build temporary expanded join indexes;
this implementation removes persistent per-fact directories, not every possible
query-time materialization.

Detailed evidence: `results.json`, `online-results.json`, and `live-allocations.json`.
Invalid early experiments with an inactive hook are marked under `out/` and excluded.
