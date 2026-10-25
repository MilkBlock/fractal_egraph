# Commit-published readiness and immutable row sharing

This adds two independently selectable refinements to the actual saved raytrace
Pass-3 execution path. They retain the same 75 admitted queries, original RHS,
original Run frontiers, full native binding audit and ordinary GPU fallback.
Neither is a new RipSat closure proof or a general negative theorem about an
incompletely baked Cell library.

## Readiness is published from committed changes

Each projection handle has an exact presence bit, backed by the complete current
native rows of that typed projection. A candidate whose conjunction has only one
distinct condition aliases that bit directly; it does not get another maintained
Boolean node. A multi-condition candidate has a stored readiness bit.

At a native commit/rebuild boundary:

1. Remember a touched condition's old presence once, then apply all its row and
   payload updates. Native changes still invalidate that payload's version.
2. Compare old and final presence. Replacing a function result or changing its
   timestamp can leave presence unchanged; those updates do not broadcast an
   existence change to dependent conjunctions.
3. Notify only consumers of the condition keys whose presence really changed.
   New or rewired anchor attachments are also refreshed, including ones created
   during a transient intermediate state in the batch.
4. Recompute affected conjunctions via their fixed condition handles.

A later query reads the published bit **before** cloning a candidate row,
allocating its temporary condition links, or reading its witness payloads.
Negative candidates therefore do not enter the Run-local grouping machinery.
The original timestamp watermark and complete witness product remain checked
for positive candidates. A function update must still produce fresh bindings
when existence remains true; readiness is not permission to skip that work.

Presence is packed into a bit vector per projection. This run uses **63,080 bytes
of logical bits / 85,280 bytes allocated capacity** for 503,794 condition handles.
These figures exclude rows, directories, anchor attachments and the native graph;
they are not RSS or graph-compression ratios. Empty historical handles are not
recycled yet. `--condition-execution ready` selects this mode; `groups` is the
prior Run-local exact-group control.

## Ownership is a separate knob

With `build.py --shared-rows`, the table, projections, prepared candidates and
cached answer lists hold references to the same immutable fact row. Updates
allocate a new row, preserving older answer snapshots until their last consumer
releases them. Sharing does not union native terms, collapse witnesses or change
any values/timestamps. The default build retains owned/cloned rows.

This is a compile-time `shared-rows` feature, not a data-dependent heuristic.
Both builds use identical sources and dependency revision. Build outputs pin a
copy of their actual executable as `BUILD/runner`, so compiling another feature
cannot silently overwrite the control. The benchmark records build provenance
and validates that storage comparisons differ only by this feature.

The legacy `witness_rows_materialized` counter measures rows listed in answer
snapshots. Under shared storage these entries are references, not fresh copies
of their value arrays. Do not interpret unchanged row counts as equal allocation
work or claim an unmeasured RSS reduction.

## Results and limits

Each timed configuration ran once, serially. Audit runs are separate. Unchanged
native and ordinary-GPU controls were reused explicitly.

**Same-binary readiness comparison, owned row storage:**

| Metric | Run-local groups | Commit readiness |
|---|---:|---:|
| Full inference | 21.1015 s | 21.1000 s |
| Maintenance | 1.3211 s | 1.4113 s |
| Query handling | 1.1741 s | 1.1130 s |
| Maintenance + query | 2.4952 s | 2.5243 s |

Readiness avoids temporary query preparation for **544,707 candidate visits**.
Of **1,404,096 coalesced condition updates**, **1,144,862** leave presence unchanged
and **259,234** change it. It refreshes **279,999 multi-condition attachments**;
**1,715,502** single-condition candidate visits alias their condition directly.
This replaces the earlier per-Run condition/conjunction checks, but is not free:
maintenance increases and the combined local phase is slightly slower. Total
time is essentially unchanged. The stored-attachment transition counter includes
initialization/replacement of attachments, not just changes of a persistent
semantic identity.

**Same-source compile-time ownership comparison, both using readiness:**

| Metric | Owned rows | Shared immutable rows |
|---|---:|---:|
| Full inference | 21.0001 s | 21.0736 s |
| Maintenance | 1.4514 s | 1.3184 s |
| Query handling | 1.1331 s | 1.1002 s |
| Maintenance + query | 2.5846 s | 2.4186 s |

The measured local phase is **6.42% lower**, but full inference is not faster in
this single pair. All ordinary runtime work/traffic counters agree. Do not claim
an end-to-end win from the smaller local phase or select a favorable timing from
the preceding experiments. Both refinements remain optional. The historical
ordinary-GPU control (17.714 s) is still faster than this provider path.

The useful outcome is a precise split between existence changes and payload
changes, and shared immutable payload ownership. It is not evidence that more
CPU side views or ever wider prediction matrices will accelerate the whole
workload. Remaining native action/rebuild and ordinary GPU work dominate.

## Validation and reuse

- **29 Rust tests in each ownership build** pass, including late facts,
  removal/reinsertion, multiple witnesses, aliases, native union/subsume,
  function updates with unchanged readiness, no-change Run frontiers, and
  selected ownership semantics.
- **1,988,917 independent readiness checks** compare published/aliased flags
  against current projected rows in the diagnostic run.
- **134,850 native incremental full-binding query checks** pass.
- Final **699,068 original rows**, values, equality and visibility match the
  unchanged common-submit native control.
- Production Tiger selects identical DAGs for **29 functions** for the shared
  candidate. This is not pristine-upstream parity.

Use the existing `run.py` flags with `--condition-execution ready`. Build either
normally or with `--shared-rows`; `run.py` reads the pinned executable from that
build's provenance. `--compare-execution PREVIOUS_RUN` reports the same-binary
mode comparison. `--compare-storage OWNED_RUN` verifies same-source feature
identity and reports the ownership pair. Both reuse completed lanes and produce
all per-rule counters/depth distributions without rerunning inference. Depth
zero for a prefiltered candidate is charged to event maintenance; it is not free
rejection or native Free Join depth.

Final directories: `out/condition-ready-{control,bits}` for readiness,
`out/condition-{owned,shared}-rows` for ownership. `out/condition-ready` is the
pre-bitmap prototype, retained separately. Full results are `ready-results.json`.
