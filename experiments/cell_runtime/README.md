# Standalone Cell CPU runtime

This is the actual `cell-cpu` executor, **not** the older egglog-backed CPU
worker, and not a GPU performance result. The standalone Rust crate links only
Serde/JSON/SHA256. Egglog is used in a separate process for typed frontend export
and independent tests; it is not called to execute this runtime's inference.

## Storage and execution

- Source RHS wiring is shared in templates. An instance stores bindings,
  computed scalar values, active fact positions, and optional parent projections.
  Constructor output identities are derived from instance/slot coordinates.
- Virtual constructor rows are decoded on demand. There is still per-fact
  metadata, hashcons reference indexing, alias indexing and subscriptions. A
  100-constructor RHS uses two frames including its input, **not two total
  storage records**. Do not infer RSS compression from the frame count.
- Mutable functions retain keyed result ports; source merge expressions, ordered
  writes, intermediate constructors, deferred union, and affected-key repair are
  executed. Subsumption affects a fact, not its whole equivalence class.
- Every Run gathers bindings from its entry state before any RHS executes.
  Repeat, nested Saturate, positive checks, until probes, checkpoint/resume and
  stage visibility are explicit control state. A partial checkpoint is never
  reported as a completed saturation.
- Single-table queries use committed fact notifications and may borrow their
  binding from a parent recipe. Multi-table residual matching is still required.

The shared compiler route is
`source.egg -> source_sk.egg -> source_sk_mk.egg -> recipe -> cell-cpu artifact`.
Use `python3 -m eggfuse compile --backend cell-cpu ...` and
`python3 -m eggfuse run --artifact ... --output ... --check`.
The latter checks reported complete LHS bindings and independent-process replay;
it does not claim equality to native egglog on every program.

## CPU optimizations

1. Start semi-naive residual queries from actually changed facts. Disjoint
   partitions use the first changed table atom; they retain all bindings while
   avoiding repeated enumeration of old-only joins.
2. Maintain per-operator function membership and use existing alias dependencies
   as a conservative query index; avoid scanning all functions for cardinality.
3. Lazily build shared column indexes, then maintain only changed references from
   committed writes and alias repair. Buckets store fingerprints, fact references,
   and per-fact old fingerprints, not copied constructor rows. Version checks
   guard publication boundaries. Collisions only enlarge candidate sets: complete
   typed binding checks remain mandatory. `CELL_CPU_QUERY_INDEX_SCOPE=run|table`
   enables full Run/table invalidation controls; default is incremental.
4. Backtrack in a dense temporary slot array using an undo trail. Only complete
   matches allocate stored bindings; failed branches no longer clone BTreeMaps.
5. Reject a query with an empty required table before starting its join. This is
   an ordinary query optimization and is not credited as Cell proof reuse.
6. Reuse immutable compiled query/RHS programs instead of cloning their operations
   per binding. Merge lookups inspect dirty facts only in the requested operator.

Cross-stage joined-key summaries and candidate refinements are documented in
[SIEVE_JOIN.md](SIEVE_JOIN.md). They are opt-in and separately measured;
ordinary table-index reuse is already present without them.

`CELL_CPU_QUERY_MODE=scan|indexed|reverse|delta` supports diagnostic ablations.
`scan` disables query indexes/delta seeding; `indexed` disables delta seeding;
`reverse` uses only the alias-reference index; default `delta` enables all.
These modes retain the same source schedule. They are not inference budgets.
Run phase times separately record search, RHS, repair, and event delivery/GC.
`CELL_CPU_PROFILE=1` additionally times individual queries; `CELL_CPU_PROGRESS=1`
logs Run starts (and query starts when profiling). Diagnostic runs are excluded
from clean performance reports.
`CELL_CPU_HASH_BITS=0` forces all fingerprint keys to collide, for correctness
tests only. All 19 native cases also pass this adversarial configuration.

## Checks and clean benchmarks

```sh
python3 -m unittest eggfuse.ir.test_cell_runtime
python3 experiments/cell_runtime/check.py --output out/cell-cpu-check

# Save clean, one-process controls. Source SHA is checked against the frozen suite.
python3 experiments/cell_runtime/suite.py --freeze --output out/cell-cpu-control
# After a change: reuse exactly those controls, no native/GPU work in the timer.
python3 experiments/cell_runtime/suite.py --baseline out/cell-cpu-control \
  --output out/cell-cpu-candidate
# Separate full per-stage/binding comparison (never count these times):
python3 experiments/cell_runtime/suite.py --baseline out/cell-cpu-control \
  --audit-baseline-engine /path/to/frozen-binary --output out/cell-cpu-audit
# Optional large workload (not a daily default): append --cases raytrace.
```

The benchmark checks exact final state, per-Run control/match counts and mutation
work against the frozen CPU implementation. Inference excludes setup, JSON
output and checkpoint serialization; process wall and macOS peak RSS are reported
separately. No audit is enabled during a timing run. `suite.py` without a mode
runs the separate full-LHS/native/replay correctness survey with audit overhead.
The existing cell-assisted bench worker is a different backend; this adapter
uses the same historical-control and single-run conventions without conflating it.

Final graph output is streamed. A paused run writes `checkpoint.json`; a completed
run writes one only with `--save-checkpoint`. The latter still builds the explicit
checkpoint image and can be memory intensive. Compare inference times separately
from this changed output policy; its RSS reduction is not Cell storage compression.

## Boundaries

Cross-rule recipes, conditional local continuation, and source-stage publication
are implemented; see [MULTISTAGE.md](MULTISTAGE.md) for contracts and measurements.
Ambiguous boundaries still use residual matching. This is not the complete
Cell-resident roadmap. GPU execution and production eggcc/Tiger extraction are not implemented
here. Unit-cost extraction is diagnostic. The frontend rejects unsupported
post-initialization commands; floating point arithmetic and some custom merge
function reads are not supported.

The pre-optimization small Pass-3 survey already differed from native egglog:
extra `Smaller` rows in fact/loopfact/sum-bits, plus equality differences in
loopfact and a missing native `TermSingle` key in sum-bits. These are **unresolved**;
do not hide them by dropping Smaller from acceptance. Before/after optimization
equivalence is a narrower claim than replacing pristine egglog correctly.

## Reference marker views and checked takeover

The official CPU target can emit exact slot projections and conditional containment certificates. `CELL_CPU_REFERENCE_VIEWS=1` enables virtual marker records, reference redirection, inherited continuation entries and body retirement. This remains opt-in: sum-bits passes every Run/binding comparison but is slower. See [REFERENCE_VIEWS.md](REFERENCE_VIEWS.md) for the representation, controls, tests and measured limits.

## Union-driven Bake workload

`CELL_CPU_UNION_IMPACT=1` observes direct union inputs separately from derived
congruence results, without changing matching. The [survey](UNION_BAKE_SURVEY.md)
reports the actual compiled library's certification status, per-template impact,
repair attribution, causal recurrence, and symbolic Bake work requests. It does
not turn a repeated coarse signature into an executable Cell transition.

Compiled ordered merges and their original-stage consumers are available with
`CELL_CPU_MERGE_PROGRAMS=1`. See [PARAMETERIZED_MERGE.md](PARAMETERIZED_MERGE.md)
for parameter programs, marker relays, native Cell checks, the displaced-only
union repair fix, and the remaining whole-Pass-3 difference.
