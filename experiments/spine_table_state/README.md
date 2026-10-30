# Table-backed resumable Spine state

This changes the actual source compiler and resident GPU lowering. It is not a
new side-cache prototype. The existing `source.egg -> source_sk.egg ->
source_sk_mk.egg -> device artifact` pipeline emits the state tables.

## Representation

* `CellSpineView*` are exact, correlated constructor views. Proper prefixes are
  interned as ordinary Unit-valued functions. A longer prefix reads an existing
  prefix and one additional constructor, rather than its whole original shape.
  An incomplete prefix persists even when its consumer's dynamic inputs are absent.
* Producer RHSs publish what their actual LHS/RHS proves. Symbolic continuations
  that branch are not enumerated into a Cartesian product of source actions:
  only a unique known extension is published inline. The exact prefix completion
  rules cover other witnesses, initial facts and union-created connections.
* `marker-program/v2` connects each consumer to its structural rows, live typed
  port queries, guards, effects and successful-application receipt table.
  Mutable values are not duplicated into a cache: the original function remains
  authoritative. The original control program owns Stage and Repeat progress;
  native table timestamps and per-consumer frontiers activate new inputs.
* `CellSpineApplied*` records a successful application's effect-input projection.
  The full correlated binding remains in the Spine/port query. Unused LHS fields
  need not widen the receipt: two bindings with identical necessary effects may
  share one record. An actual source-derived proof must cover every observable
  input before a receipt is allowed to suppress work.
* Only immutable, visible, default-merge constructor effects receive a reusable
  completion proof. Their complete RHS can be skipped after exact current LHS
  and guard evaluation. General set/union/merge receipts are historical evidence,
  never permanent done bits. New values continue through the original rules.

There is no separately maintained persistent Spine binding cache in this path.
The marker tables participate in ordinary canonicalization, indexing and
semi-naive execution. This does **not** eliminate the remaining marker/port
queries or implement an arbitrary whole-Cell completion theorem. Initial and
union-created unseen bindings still require exact incremental discovery.

## Admission repair

The saved Pass-3 compiler had 72 observed Cells but no matching offered view,
so its marker solver received zero variables. Bake evidence is now retained as
provenance, not a veto on a source-provable positive view. Such a view grants no
RipSat closure certificate. Connected reverse fibers retain all correlated
coordinates. Single-consumer candidates may be selected by lifecycle prices;
the default structural score does not force them into the program.

Markers over explicitly subsumed/deleted source tables remain excluded. Mutable
ports and numeric guards stay live. Reserved namespaces, complete view proofs,
receipt projections and source control are validated on loading. Legacy v1
artifacts remain readable.

## Verification

Run with the existing SCIP environment (no new dependency installation):

```sh
/Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python \
  -m unittest eggfuse.compiler.test_spine_states -v

/Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python \
  experiments/spine_table_state/check.py \
  --build out/eggfuse-clean-paths-build --output out/new-spine-state-check
```

The script independently compares original and rewritten native tables, then
compares the rewritten program with the resident GPU, and repeats the GPU run
in a fresh process. It covers Boolean literals, late and updated ports, union,
subsume, finite Repeat, non-idempotent effects, repeated visits and entry from
an existing intermediate Spine without executing its initial producer.

In the naive-revisit fixture, consumer bindings are **[2, 2, 2] with receipt
shortcuts disabled**, **[2, 0, 0] enabled**, with the same complete final state.
This is a work-count test, not an end-to-end performance claim.

`check.py --pass3-source SOURCE_SK.egg --analysis SAVED_ANALYZE_DIR` additionally
runs the real Pass-3 marker selection, native typechecking, contract validation,
region analysis and resident device lowering using saved Analyze evidence.
It explicitly does not rerun full Pass 3 or claim a speedup. See `results.json`
for the retained acceptance results.

A wide-receipt draft increased the global padded row stride from 25 to 40 words.
Effect-input projection removes that regression: the accepted generated Pass-3
layout retains the original 25-word stride. Additional marker/receipt row counts
and their runtime cost still require full workload measurement; table layout
compatibility alone is not memory compression or a throughput result.

## Saved Pass-3 compiler result

The accepted compilation offers 308 connected fragments (14 with a witness in
the saved Bake sample), selects 48 structural/prefix tables, rewrites 112
consumers and adds 707 producer publication actions. It exports 112 consumer
state contracts; 22 qualify for persistent immutable-effect receipt skipping.
All remaining consumers retain exact effects. Marker admission and state types
are not counts of runtime Cell instances or proven saturated Cells.

The same acceptance run passes eight resident GPU fixtures and the 39-test
eggfuse unit suite. The unified compiler suite separately passes source-to-device
execution, native projection, artifact loading without re-lowering, pass cache
reuse/invalidation and rejection of modified guarantees/fields/control/guards.
