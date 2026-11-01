# Checked marker references and Cell storage takeover

Implemented in the existing standalone CPU Cell executor and official CPU compiler target. The native egglog execution engine and Metal backend are not modified. The feature is **opt-in** (`CELL_CPU_REFERENCE_VIEWS=1`). The historical provider-chain implementation regressed on sum-bits; later field-copy and maintenance optimizations are recorded below. This is not a claim of increased RipSat coverage or native egglog acceleration.

## Compiler contract

The device pass emits `reference-views.json`, embeds the same contract in `program.json`, and binds it to the typed recipe checksum. The marked source is unchanged. Its ordinary logical marker columns remain available to queries and to the native reference implementation.

The contract describes:

- Exact query-atom/column projections for input slots. Mutable function results are copied into the binding snapshot rather than referenced as changing inputs.
- Carrier rows: already matched immutable rows that the target recipe can represent, including non-root positions.
- Shared slot accessors for the original producer's continuation program, so an absorbed producer can still supply its entry points.
- Storage roles. Ordinary rule instances own their parameters. Pure marker publishers may use referenced providers or retain a local field copy; mixed publishers reuse the ordinary owner's parameters without a second function-key tuple.
- Selected marker tables. Only inert Unit functions with `merge old`, no RHS reads/removals and no merge-callback readers can use the reference representation. The normal pipeline also validates its full marker IR.

Rust independently reconstructs and validates the projections/accessors against the typed source recipe. A forged coordinate, carrier or storage role is rejected. The compiler supplies a **conditional containment certificate**, not a claim that every producer instance is covered: runtime must show that every current owned fact and retained parameter position fits the target carriers.

## Runtime representation

A marker row is an ordinary query-visible virtual fact reference. With
`CELL_CPU_MARKER_FIELDS=reference`, its fields are recovered from provider recipes:

```
FactRef { cell, operation }
        ↓
shared recipe + typed slot projection
        ↓
provider FactRefs, possibly from several Cells
```

It no longer needs a separate explicit `Function.key` tuple. In reference mode,
pure views do not store their projected input values. Coordinates not proven
shareable remain stored. Normal rules still see the same complete correlated
binding; equality-class alternatives are not collapsed to one representative.

The default within the opt-in storage path is now `CELL_CPU_MARKER_FIELDS=copy`:

```
FactRef { cell, operation }
        ↓
shared recipe + local matched field tuple
        ↓
canonicalize equality IDs through union-find
```

This keeps virtual fact identity and optional storage takeover, but removes the
provider chain and its pins from pure marker frames. It does not expand a Cell
into all its table rows, change the source marker schema, or turn markers back
into explicit `Function.key` records. Copies contain immutable matched values,
not frozen canonical identities: `slot()` still applies `canon`. `activate`
subscribes the marker's complete row to equality dependencies; union marks the
fact dirty, repair recalculates its canonical key, coalesces duplicate marker
facts and notifies indexes. Mutable function inputs remain binding snapshots.

Checkpoint images record the field-storage mode and reject a mode change during
resume. Old reference-only checkpoints can resume with the explicit `reference`
setting. Copies of fields are distinct from copies of a Cell's derived closure.

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

28 tests pass, including streaming checks of the existing 52 regression programs. Dedicated cases cover multi-owner markers, alias-created matches, late inputs, finite Repeat, hidden/deleted/recreated facts after takeover, partial coverage, retained mutable function ownership, inherited compiled entries, checkpoint resume after takeover, and a forged slot certificate. Checkpoint tests verify that reference mode omits projected fields, copy mode retains them without provider pins, union/repair still gives identical per-stage graphs/bindings, and resume cannot silently switch the storage mode.

## Historical sum-bits Pass 3 (referenced fields)

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

## Field-copy ablation (2026-11-01)

The compiler artifact above is unchanged. All five lanes below were measured
once using binary `30af72aa3c7963d331839e2e6bc93fc6615b0f28c824841dd2df8df073e45ddd`.
Source markers and the original schedule are present in **every** lane. Control
disables only reference-view storage and takeover; it is not a no-marker run.
This is the standalone Cell CPU engine, not a pristine egglog benchmark.

| Storage and takeover | Inference | Search | RHS | Repair bucket |
|---|---:|---:|---:|---:|
| Original marker storage, no takeover | 0.5535 s | 0.2907 s | 0.0788 s | 0.0802 s |
| Referenced fields, no takeover | 1.1306 s | 0.6292 s | 0.1567 s | 0.1485 s |
| **Copied fields, no takeover** | **0.8405 s** | **0.4530 s** | **0.0952 s** | **0.1290 s** |
| Referenced fields, takeover | 1.5408 s | 0.6430 s | 0.1668 s | 0.5378 s |
| **Copied fields, takeover** | **1.2327 s** | **0.4583 s** | **0.0984 s** | **0.5143 s** |

Copying fields saves 0.2901 s (25.7%) without takeover and 0.3081 s (20.0%) with
takeover. These are whole-mode differences, not an isolated timer for pointer
chasing. Provider setup/validation, pin maintenance, field decoding and their
downstream costs change together. The repair bucket still includes takeover
and pending-event retargeting; it is not a measurement of rebuild alone.

All four candidates independently pass **1,623 per-Run full-graph and complete
binding-sequence comparisons** against control. All clean runs finish with
10,339 logical records, 45,911 complete bindings and 1,089 unions. Corresponding
reference/copy lanes have identical candidate visits, decoded-row counts and
repaired-dependency counts. Both takeover lanes retire 946 bodies and relocate
950 facts; the faster copy lane does not disable takeover to obtain its result.

Without takeover, copying changes stored Cell parameter values from 20,269 to
51,889, and provider handles from 12,098 to zero. With takeover, values change
from 17,963 to 48,743 and provider handles from 11,858 to zero. Peak RSS spans
221.1–224.1 MiB across these single runs; this is not evidence of a significant
RSS improvement or regression.

The old FactRef representation and the new provider representation are not the
same read path. FactRef names a fact position. Referenced marker *fields* add a
second level: slot projection, provider FactRef, provider recipe column, possibly
another provider. Each step has map/recipe lookups and lifecycle bookkeeping.
Copies keep the fact handle but terminate most marker field reads at local
parameters. They still canonicalize equality IDs and subscribe to repair.

The copied/no-takeover path remains 0.2870 s slower than control. The source
confirms that general match-witness collection/cloning remains enabled, and
virtual marker facts still use recipe decoding and a different key/repair path
than explicit Unit function records. These remaining costs have **not** been
separately timed. Takeover adds another net 0.3922 s in copy mode, mostly in the
combined repair bucket. No claim is made that copies fix the entire regression.

Reproduce without rebuilding Bake or solver output:

```sh
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.reference_views_bench \
  --compiled out/reference-pass3-owned/compile \
  --output out/new-marker-field-comparison --marker-field-ablation
```

The helper verifies official pipeline receipts, runs correctness audits separately,
then measures each clean lane once. Provenance, configuration, work counts and
phase times are retained in `reference-view-results/marker-field-copies.json`.

## Removing the storage regression

The optimized copy path retains the same logical marker schema, complete Run
schedule, union semantics and exact containment checks. It also retains generic
matched-carrier takeover; no successful takeover is disabled to improve timing.

- **Packed marker fields.** Keep a contiguous, typed copy of the marker row at
  its FactRef. It is additional storage alongside recipe parameters, explicitly
  reported as `reference_views.inline_marker_values`. This caches marker rows,
  not every derived constructor in a Cell.
- **Canonical-state validity.** A packed row is read directly while canonical.
  Reverse union dependencies invalidate it immediately. Dirty reads canonicalize
  before use, and repair refreshes the packed row. Checkpoints omit this derived
  cache and rebuild it from checked recipes on restore.
- **Repair at the right point.** Only compiler-validated inert Unit markers with
  pure `merge old`, no RHS read/removal and no merge readers use a separate dirty
  queue. They are repaired after ordinary constructors/functions reach their
  repair fixpoint, but before the same Run is published. These markers cannot
  themselves create a union or change a merge callback result. This avoids
  repeatedly repairing their keys during intermediate congruence merges. The
  fallback and restrictions for other functions are unchanged.
- **Minimal match provenance.** Copied-field takeover only records source Cell
  IDs per query atom, using compact `u32` slots. Full FactRefs are retained when
  provider-column projection needs them. With copies and takeover disabled,
  matching records no storage provenance. Logical bindings are unchanged.
- **Exact early rejection.** Check whether target carriers can represent every
  required source table before expanding any target parameters. Build canonical
  target carrier tuples once per target, reusing them for its source owners.
  No budget, heuristic cutoff or weaker containment condition is introduced.
- **Affected work only.** Fact relocation retargets pending handles only in
  readers of that fact's table. Run-boundary delivery deduplicates repeated
  notifications and reads liveness/table metadata without decoding full rows.
  Matching still sees the original committed Run boundary.

General reads also reuse the already-found Cell while decoding columns, avoid
double canonicalization and avoid copying a hash bucket for a clean keyed read.
These common improvements benefit both comparison lanes and are not Cell credit.

For optimization iterations, the existing helper accepts `--copy-only
--timing-only` for a fast final-graph check and clean timing. This is explicitly
not per-Run correctness acceptance. After selecting an unchanged binary,
`--copy-only --audit-only` performs the full graph/binding audit and records the
program and binary fingerprints without rerunning already measured timings.

### Verified sum-bits result, 2026-11-02

Binary `31a3adfc88b105e8b75511e85917cdde19f873ecd1965c2dfafea31cc0c7ff96`,
same `7ba5d673...` official compiler artifact and source markers as above:

| Configuration | Inference | Search | Repair bucket | Peak RSS |
|---|---:|---:|---:|---:|
| Original marker storage (same binary) | 0.507630 s | 0.270926 s | 0.077943 s | 225.56 MiB |
| Optimized copies, no takeover | 0.467340 s | 0.240056 s | 0.077039 s | 228.86 MiB |
| **Optimized copies, takeover retained** | **0.500984 s** | **0.247346 s** | **0.097088 s** | **227.66 MiB** |

The previous large regression is absent on this workload in the single paired
measurement. The takeover lane is 1.3% below control; the difference is small and
does not establish a substantial net speedup. Compared with the previous
field-copy implementation's historical 1.232698 s, the retained-takeover path is
now 0.500984 s. Common-engine improvements also speed up control, so that whole
historical difference is not attributed solely to Cell storage.

Both candidates independently pass all **1,623 Run-boundary full graphs and
complete binding sequences** against control. All retain 10,339 final logical
records, 45,911 complete bindings and 1,089 unions. Takeover still absorbs 946
bodies, relocates 950 facts and preserves all 6,497 marker views. 17,452 of the
19,888 containment attempts are rejected by exact table coverage before tuple
expansion; the accepted/rejected totals are unchanged.

In no-takeover mode, delaying inert marker repair reduces repaired-dependency
visits from the earlier 50,934 to **38,291**, equal to control. With takeover the
count is 38,283. Its 52,383 pending notifications are deduplicated to 45,972 final
fact notifications at the same Run boundaries. These are maintenance counts,
not skipped logical matches or additional closure proofs.

The extra packed rows contain **40,097 Value slots** alongside 48,743 owned Cell
parameter values in the takeover lane. Their cost is reported; they are not
called free storage. The observed peak RSS increase over control is about
2.1 MiB, from one process measurement per lane.

All 28 tests pass, including the existing 52-program streaming suite. The
additional checks cover reference-mode compatibility, cache reconstruction on
checkpoint restore, repeated-notification coalescing and rejection of observable
marker removal. The native frontend and native egglog runtime were not modified.

Clean timing preceded the final full audit. During validation the worktree build
directory disappeared; an offline rebuild reproduced the **identical binary
hash**. The existing clean timings were therefore reused, not resampled. Inputs,
binary hashes, phase times, work counts, RSS and full-audit results are recorded
in `reference-view-results/marker-maintenance.json`. Earlier exploratory runs,
including a candidate that still regressed by 7.5%, were not selected as the
final implementation and remain under `out/reference-regression-*`.
