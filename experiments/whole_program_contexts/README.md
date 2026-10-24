# Whole-program context and path-retention selection

The previous eight-query experiment manually selected the consumers before
solving. It could not discover sharing with consumers outside that list. Also,
the runtime enabled private `PortPath` retention for every selected query; that
was an executor default, not a solver discovery.

This change exposes the full scheduled query set and makes private path
retention an explicit plan choice. It does not make the solver synthesize a new
cache implementation or claim that the present cost model optimizes elapsed time.

## Domain and decisions

`context-domain.json` is derived from every native-admitted query in the main
schedule, with no special case for a rule name, DoWhile, or a count of 84. It
contains **480 queries, 4,462 typed prefixes and 604 multi-atom prefixes used by
multiple consumers**. Each use retains its exact local slot map. Types,
constants and repeated-variable constraints survive normalization; consumer
guards remain outside the shared prefix. Parent links describe prefix extension.
This is query-context sharing, not a new RipSat saturation certificate.

For each measured, nonempty query, the selector chooses among rooted plans and:

1. Read without a saved private path.
2. Save a private path and reuse unchanged reads.
3. Materialize a prefix with a selected payload for its consumers.

The existing joint objective charges shared definitions once, constrains each
consumer to one implementation, and charges private state separately. Private
path fields and writes are now included instead of assuming retention is free.
The `cache_ports` decision is serialized, validated, applied by the runtime and
preserved in portable libraries. Older files default to their previous behavior.
Dependencies and committed-table mirrors remain common runtime machinery even
when private paths are disabled.

The original workload is reused: **96 inputs / 288 passes / 411,633 Run
frontiers**. Re-aggregation adds per-query empty-input observations. A query
whose input is empty at every observed frontier retains the ordinary executor,
which already performs that short circuit. This does not claim Cell credit and
does not suppress future matches: the original query remains executable.
No new full-corpus inference or trace capture was required.

The result has **332 selected query implementations**: 39 private with saved
paths, 243 private without saved paths, and 50 consumers of 51 materialized
prefix definitions (some definitions are parents or have only one consumer).
The remaining 148 admitted queries retain ordinary matching because of empty
or missing workload evidence; unsupported/protected queries are also unchanged.

The `lsr-inv` loop query independently chooses a DoWhile root **and saved private
paths**. There is no `if rule == lsr-inv` optimization branch. On raytrace it
creates 84 live paths and reuses 2,814 reads, matching the old hand-selected
behavior. The number of paths comes from runtime facts. Other automatically
discovered structures include typed loop prefixes and common term/context
queries; their mere presence is not a performance result.

## Actual-runtime counterfactual

All three lanes below use the same new executable, frozen worker, full saved
raytrace Pass-3 input, schedule, choice policy, selected roots and query family.
Each is run once without diagnostic counters. Existing native/CPU graph controls
are reused and newly checked. Setup, solver, export and extraction are excluded.

| Lane | Inference seconds | Final private path entries |
|---|---:|---:|
| Same selected plan, force private retention on | 13.442816458 | 423,004 |
| Solver-selected retention and materialized prefixes | 13.324833500 | 84 |
| Same solver plan, shared prefixes disabled | 13.283449625 | 5,155 |

The 0.88% paired timing difference is small. The clear result is removal of
unnecessary private path entries, not an equivalent RSS reduction. These counts
exclude shared prefix frames, subscription indexes and other runtime storage.
Selected uncached queries all report zero saved paths and zero reused port reads.

Shared prefixes still have **no demonstrated timing gain** in the final pair:
13.28345 -> 13.32483 seconds. Before adding the cache choice and excluding
already-empty work, whole-program selection produced 86 definitions and actually
regressed from 12.29941 to 13.89221 seconds with sharing enabled. Those earlier
lanes use a different binary and are not controls for the final pair. Neither
result is promoted as a default fast path or hidden by reporting cache hits.

All final lanes match the same-submit CPU's original facts, values, equality and
visibility: **699,068 rows / 524,631 mapped e-classes**. A separate audit checks
**264,621 Cell-supplied queries**. Production Tiger selects the same DAG for
**29 functions** on the solver-selected lane. The pre-existing fused-native
discrepancy remains; this is not a pristine-equivalence claim.

See [results.json](results.json) for fingerprints, configuration, counts and
links to full skill-generated counterfactual reports. The skill entry points are
`spine-program --runtime-workload ...` (without a query subset),
`boundary-credit --layouts`, and `shared-marker-credit`.

## Remaining limits

* The plan domain still uses the native compiler's fixed order for each root.
  It is not arbitrary join-order or arbitrary fragment synthesis.
* Private search work is measured. No-path lookup work and path-storage/write
  costs are derived estimates; terminal delta pruning can make no-path work
  smaller. Shared maintenance also remains estimated from the corpus census.
* Direct RHS publication is implemented separately, but its avoided repair work
  is not yet included in this objective.
* Choosing CPU versus the original GPU query is not priced. Private-without-path
  is still a compiled CPU provider, not an automatic GPU placement decision.
* The common mechanisms (cache, subscriptions, prefix materialization) are
  implemented operators. The solver chooses their uses; it does not invent Rust.

These limits explain why automatic discovery can now produce real contexts and
remove excess state without yet finding the best end-to-end execution plan.
The full-program domain also exposed excessive AST duplication in model
construction; shapes and normalized descriptions are now reused immutably,
without removing any semantically distinct runtime plan.

Validation includes typed alias/renaming discovery tests, measured-work cache
choice tests, native function-update/union/delete/subsume checks for both cache
modes, portable no-path reload, 19 generic-transfer regressions, and the bench
comparison checks that preserve non-boundary cache/proof settings.
