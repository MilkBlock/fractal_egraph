# Cross-stage Sieve-style filtering and Free Join-style refinement

This extends the **actual standalone Rust Cell CPU executor**. It does not link
native egglog's Free Join implementation, change the GPU backend, or reproduce an
entire published SieveJoin engine. Native egglog already shares table-column
indexes across rulesets (`egglog/core-relations/src/free_join/mod.rs`,
`execute.rs::get_index`). The new reusable object is a **filtered fragment key
domain**, plus a demanded candidate refinement, not another copy of those column
indexes. Both features are explicit experimental options:

```sh
CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1 \
  engine/cell-runtime/target/release/eggfuse-cell-runtime \
  --program path/to/program.json --output out/experiment
```

The verified Recipe IR and original control tree are unchanged. A finite Run
still matches against one immutable visible snapshot and then publishes RHS,
repairs identities, updates query indexes and propagates filter changes. Filters
are not reset when moving to a different ruleset. Checkpoints reconstruct these
query accelerators from the restored visible graph, not from serialized stale
negative answers.

## What is shared

For `P(x,a), Q(a,b), T(b,c)`, construct:

```
T.b                             complete visible T keys
  ↓
Q.a where Q.b may occur in T.b   a joined-fragment summary
  ↓
reject P rows whose a is absent, before searching Q and T
```

A second rule in a different ruleset using the same `Q → T` fragment reuses the
same summary. Specs are interned by table, projected columns and child summaries;
source variable names and ruleset names are not part of identity. Composite keys
retain every shared parameter, so `(owner, index)` does not collapse into owner.

The compiler picks one deterministic maximum-shared-column spanning forest of
the query atoms. It emits both directional messages per edge. The number of
messages is linear in atom count; it does **not** enumerate all paths or all
possible Cell combinations. Non-tree constraints, literals, repeated variables
and primitive guards remain in exact matching. Ignoring them in the filter can
only weaken rejection. A bare single-table membership/empty-table check is not
counted as a joined-fragment optimization.

Free Join-style refinement starts from the smallest already-bound column bucket
and intersects its witness references with the other bound-column fingerprints,
without decoding virtual rows. Nonempty proper refinements are cached under the
bucket and selector tuple, shared by all consumers. Empty lookups and identity
refinements are not cached. Changing any column removes/reinserts the witness in
all its owning buckets, invalidating exactly those cached refinements. The final
matcher still checks typed values; fingerprints are never equality proofs.

## Incremental validity

1. Build a demanded filter from **all visible input rows**, not just Bake hits.
2. At publication, update the canonical column index first. An epoch-only change
   with the same key tuple does not refresh Sieve summaries.
3. New or rekeyed rows update the summaries reading that table.
4. A newly admitted child key looks up only related parent rows through the
   reverse column indexes. Propagate to a fixed point of the finite message DAG
   before allowing the next query to reject candidates.
5. Retired rows/keys may leave stale Bloom bits and key entries. These produce
   false positives only; exact visibility and equality checks remain mandatory.

The no-false-negative argument is inductive on the message DAG: every current
base key is inserted; every row of a real join has all its required child keys.
Either its row insertion or the last newly admitted child key schedules its
projection. Thus every complete binding passes every necessary filter, even
with Bloom and fingerprint collisions. False positives cannot produce effects
without the full original LHS check.

The implementation tracks admitted key tuples independently of Bloom bits.
**An insertion that changes no bit must still propagate its new key.** Blocks
combine insertions with bitwise OR; resizing rebuilds the layout from retained
keys. Arbitrary Bloom arrays with different key projections or layouts cannot be
ORed. Eclass union is not just OR either: affected canonical keys must be rekeyed
and published first. The current graph repair already supplies those changes.

## Costs and limits

This is general query reuse over complete visible facts. It is **not evidence
that Cell closure proofs alone deserve the measured credit**. Local Cell
continuations and compiled entry reads remain independently enabled.

Bloom lookups read projected fingerprints directly, without allocating a key
vector per candidate. Lazy summary construction, reverse propagation, and cache
maintenance are all included in inference time. `report.json` records probes,
rejections, build/update/reverse-propagation rows, retained keys and refinement
hits. The legacy `decoded_rows` counter is a matcher row-visit proxy: it also
counts already decoded delta rows when visited as pivots and excludes index
maintenance decoding. It is **not** a count of all physical row decoding.
Use residual candidate visits plus measured stage time for work attribution.
Byte payload counts exclude hash-table/vector overhead; use process RSS to
assess memory.

The summaries retain stale keys across deletion, function replacement and union.
Long mutable workloads can therefore accumulate historical keys and lose filter
selectivity. Reversible counting/retraction or an explicit version-compaction
scheme is future work, not an invisible memory budget. Refinement storage also
depends on the selector masks demanded by the workload. Cyclic joins still need
exact residual checking; no universal speedup or bounded memory independent of
history is claimed.

## Reproduction and evidence

Use the existing `check.py` and `suite.py`, rather than separate timing scripts.
Controls record binary/program hashes, flags, machine, wall/inference/setup time
and peak RSS. The suite verifies full final state, every Run's control path,
match count and effect counts. Full stage-state/binding audit is a separate run.
The audit adapter now preserves the frozen control's saved configuration.

```sh
cargo test --offline --manifest-path engine/cell-runtime/Cargo.toml
CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1 \
  python3 experiments/cell_runtime/check.py --output out/join-check
CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1 CELL_CPU_HASH_BITS=0 \
  CELL_CPU_SIEVE_COLLISIONS=1 \
  python3 experiments/cell_runtime/check.py --output out/join-collision-check
CELL_CPU_SIEVE=0 CELL_CPU_JOIN_INTERSECTION=0 \
  python3 experiments/cell_runtime/suite.py --freeze --output out/join-control
CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1 \
  python3 experiments/cell_runtime/suite.py --baseline out/join-control \
    --output out/join-both
```

For real Raytrace, append `--cases raytrace` to freeze/benchmark, or reuse the
frozen current source program with the same `timed` and `benchmark` adapters. Do not compare these CPU seconds to the older GPU
12-second configuration. Existing pristine-native whole-graph discrepancies in
the standalone CPU backend remain documented in `MULTISTAGE.md`.

## 2026-10-31 result

All following figures are one clean run per configuration, same final binary and
source program. Baseline has both new switches off and existing multi-stage
execution on. No mean or best-of-many selection is used.

| Workload | Baseline | Sieve + refinement | Reduction |
|---|---:|---:|---:|
| fact Pass 3 | 22.409 ms | 19.357 ms | 13.6% |
| loopfact Pass 3 | 22.329 ms | 19.009 ms | 14.9% |
| sum-bits Pass 3 | 163.534 ms | 138.475 ms | 15.3% |
| Raytrace Pass 3 | 76.061 s | 67.442 s | 11.3% |

Raytrace residual candidate visits fell 57,286,965 → 31,044,788 (45.8%). The
Sieve performed 36,582,252 membership probes and rejected 23,508,069 candidate
visits. These are not unique bindings or committed mutations. All 4,084,636
logical matches and effect counts remained identical. All 11,057 Run records
and the final whole graph matched the off control.

The final configuration built 759 summaries, retaining 1,542,702 projected keys;
Bloom arrays use 4,253,704 bytes and raw key payload 13,177,352 bytes, excluding
allocation/index overhead. Refinement cache has 9,500 live entries with 10,415
references, 18,497 hits and 222,489 avoided candidate visits. Peak process RSS
was 2.517 GiB off / 2.402 GiB on. These single-process peaks do not establish a
storage-compression claim.

The on-run search phase was 35.693 s versus 46.602 s off. Query maintenance
increased from 3.615 s to 5.135 s. Discarding unchanged-key notifications reduced
Sieve update-row checks from 20.97M in the first version to 10.87M; probes now
hash projected columns without allocating temporary vectors.

Sieve-only Raytrace took 72.904 s. On the small workloads, refinement-only took
22.314 / 23.087 / 163.592 ms: essentially no independent gain. The Raytrace
variant also differed in RHS, repair and GC time; do **not** attribute its entire
5.46-second gap to refinement. The robust work evidence is the large reduction
in candidate visits; component timing attribution needs more than these single
runs. Both switches remain opt-in.

Validation: 39 native endpoint / independent-LHS / fresh replay cases, the same
39 with forced fingerprint and Bloom collisions, seven checkpoint cases, eight
IR tests, two Bloom unit tests, explicit shared-cache/invalidation assertions,
and Run/table-scope invalidation controls. The three small real Pass-3 programs
also match every stage state and full binding list with both switches off/on.
This is stronger than checking only counts, but does not assert pristine-native
equality for the existing large Pass-3 CPU backend.

Reproducible text evidence is in `sieve-results/`. An initial audit attempt
correctly rejected a mismatched baseline binary hash; the accepted audit uses
the final same-binary frozen controls. Preliminary overlapping diagnostic/timing
runs are excluded from this final measurement set.
