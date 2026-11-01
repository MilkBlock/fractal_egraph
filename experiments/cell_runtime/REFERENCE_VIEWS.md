# Checked marker references and Cell storage takeover

Implemented in the existing standalone CPU Cell executor and official CPU compiler target. The native egglog execution engine and Metal backend are not modified. The feature is **opt-in** (`CELL_CPU_REFERENCE_VIEWS=1`): the correctness path is complete for checked containment, but the measured implementation is slower on sum-bits. This is not a claim of a faster backend or of increased RipSat coverage.

## Compiler contract

The device pass emits `reference-views.json`, embeds the same contract in `program.json`, and binds it to the typed recipe checksum. The marked source is unchanged. Its ordinary logical marker columns remain available to queries and to the native reference implementation.

The contract describes:

- Exact query-atom/column projections for input slots. Mutable function results are copied into the binding snapshot rather than referenced as changing inputs.
- Carrier rows: already matched immutable rows that the target recipe can represent, including non-root positions.
- Shared slot accessors for the original producer's continuation program, so an absorbed producer can still supply its entry points.
- Storage roles. Ordinary rule instances own their parameters. Pure marker publishers use referenced providers; mixed publishers reuse the ordinary owner's parameters without a second function-key tuple.
- Selected marker tables. Only inert Unit functions with `merge old`, no RHS reads/removals and no merge-callback readers can use the reference representation. The normal pipeline also validates its full marker IR.

Rust independently reconstructs and validates the projections/accessors against the typed source recipe. A forged coordinate, carrier or storage role is rejected. The compiler supplies a **conditional containment certificate**, not a claim that every producer instance is covered: runtime must show that every current owned fact and retained parameter position fits the target carriers.

## Runtime representation

A marker row is an ordinary query-visible virtual fact reference:

```
FactRef { cell, operation }
        ↓
shared recipe + typed slot projection
        ↓
provider FactRefs, possibly from several Cells
```

It no longer needs a separate explicit `Function.key` tuple. Pure views do not store their projected input values. Coordinates not proven shareable remain stored. Normal rules still see the same complete correlated binding; equality-class alternatives are not collapsed to one representative.

This removes duplicate field storage, not the logical existence record or all indexes. It also does not compress a combinatorial set of distinct bindings into one record.

## Takeover and retirement

Candidate sources come from actual successful match witnesses, not a whole-graph search or arbitrary Cell pairing. At the end of the original Run, after RHS and repair:

1. Check every source-owned live fact and every still-referenced old position against the target's carrier rows, including the complete canonical tuple.
2. Retain the source if any position is not represented. Retain mutable function owners as well: a structural row proof does not justify replacing their update provenance.
3. Detach the target's parameter links before reversing ownership, preventing reference cycles. Necessary parameter values are copied once into the new owner; the old body is not cloned and retained.
4. Move fact-position state into the new owner, preserving `epoch`, `born`, `hidden`, identity and hashcons key. No `subsume`, logical insertion or `changed=true` is generated merely by relocation.
5. Redirect stable old FactRefs, transfer pin counts, update query/rebuild indexes and retarget pending rule events.
6. Preserve the original producer's compiled continuation through a small descriptor `(template, depth)` and shared slot projections. Missing projected slots retain exact residual matching. A live marker is not treated as a saturation certificate.
7. Release the old Cell body only when no facts or ownership pins remain. Old identity aliases and lightweight continuation descriptors may remain; they do not contain a second parameter tuple or closure.

The redirection works for non-root facts and for parameter references to retired fact positions. Subsume/delete continue to affect the correct visible fact. Function writes keep their original merge contracts. Finite Run/Repeat visibility is unchanged.

Checkpoints store provider handles, forward references and retired continuation descriptors. Restore validates the actual reference DAG rather than relying on parent IDs always being older. Storage mode cannot be changed while resuming a checkpoint.

## Validation and reproduction

The normal `source.egg → source_sk.egg → source_sk_mk.egg → device` pipeline emits the contract. To reuse an existing verified compilation and rerun only the device lowering:

```sh
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.reference_views_bench \
  --pipeline /path/to/verified/cell-cpu-pipeline \
  --output out/reference-view-comparison
```

This copies the verified upstream pass receipts/artifacts and executes the official CPU target. It preserves source hashes, structural keys, marker selection, merge policies and schedule. It does not rebuild an old raw `source_core` through a separate benchmark path, rerun Bake, or invoke a new solver.

After engine-only changes, reuse the newly emitted pipeline:

```sh
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.reference_views_bench \
  --compiled out/reference-view-comparison/compile \
  --output out/reference-view-engine-comparison
```

The executable's `--verify-reference-views` runs both modes in one process with separate graphs, compares complete graph states and full bindings at every Run, and immediately releases diagnostic snapshots. Clean timed runs are separate processes and contain no snapshot audit. The comparison is against the same standalone CPU engine with the storage feature disabled, **not pristine egglog**.

Relevant tests:

```sh
EGGFUSE_TEST_FRONTEND=/path/to/eggfuse_frontend \
/opt/homebrew/bin/python3 -B -m unittest \
  experiments.cell_runtime.reference_views_test \
  eggfuse.ir.test_cell_runtime \
  experiments.cell_runtime.test_congruence_bench -v
```

25 tests pass, including streaming checks of the existing 52 regression programs. Dedicated cases cover multi-owner markers, alias-created matches, late inputs, finite Repeat, hidden/deleted/recreated facts after takeover, partial coverage, retained mutable function ownership, inherited compiled entries, checkpoint resume after takeover, and a forged slot certificate. A checkpoint test checks that referenced marker fields are actually absent from the view's owned parameter map.

## Measured sum-bits Pass 3

Final selected binary: `8f83693bcd5ca7e94c6069f3e896beb32249f50f32205c8f20f94c0010c17d0f`.
Program: `7ba5d6732f535bf5aa5052b46506c7a8f8f3fc92fdf05b51f163c771a5fd0730`.
Original source/keys/marker pass provenance is retained in `reference-view-results/sum-bits.json`.

All **1,623 Run boundaries**, full graphs, full binding sequences and logical Run changes match. No tables are excluded, including markers and receipts. Final facts/visibility/equality identities match exactly. This does not establish equality with pristine egglog or independently remeasure extraction quality.

| Metric | Disabled | Enabled |
|---|---:|---:|
| Inference | 0.5644 s | **1.5708 s** |
| Peak RSS | 225.39 MiB | 224.05 MiB |
| Live Cell bodies | 9,108 | 8,430 |
| Stored Cell parameter values | 51,889 | 17,963 |
| Explicit function records | 7,262 | 765 |
| Virtual fact records, including markers | 3,077 | 9,574 |
| All logical records | 10,339 | 10,339 |

The enabled run has 6,497 live marker views. It physically retires 946 source bodies over the run, relocates 950 facts and visits 2,990 compiled continuation entries through retired producer descriptors. This is an entry-visit count, not a count of successful matches or saved queries. The dedicated late-consumer test separately verifies that a retired producer supplies a previously unbound parameter through its projection without an unavailable-source fallback. These cumulative retirements are not the same metric as the final body-count difference. 18,483 attempts retain incompletely covered sources; 459 retain function owners. Provider references and lightweight aliases are reported, not counted as free storage.

Cell parameter values fall by 65.4%, **not RSS by 65.4%**. The roughly 0.6% RSS difference from one run is not evidence of meaningful memory compression. The small graph and common compiler/program structures dominate this measurement.

| Phase | Disabled | Enabled |
|---|---:|---:|
| Search | 0.2953 s | 0.6422 s |
| RHS | 0.0806 s | 0.1662 s |
| Repair + takeover + event retargeting | 0.0844 s | 0.5648 s |
| Query-index update | 0.0254 s | 0.0596 s |
| Delivery + GC | 0.0695 s | 0.1283 s |

The current representation reduces persistent tuple duplication but increases reference decoding and maintenance work. Repair/takeover are not independently timed, so their individual contributions are not asserted. There is no added closure skipping, reduced marker-completion search, or new marker coverage in this change.

An attempted per-read batching decoder showed no net benefit (0.5451 → 1.6300 s); it was removed. The final binary hash matches the already verified selected measurement, so that measurement was reused rather than repeated. The earlier all-instance reference-chain version is likewise not the shipped layout: ordinary instances remain parameter owners.

Remaining limits are explicit: no automatic absorption of mutable function owners; no retirement from weak domination or future-stage derivability alone; no global enumeration of every possible sub-Cell; lightweight forwarding/continuation metadata is retained; and no default enablement until actual workload benefits justify it.
