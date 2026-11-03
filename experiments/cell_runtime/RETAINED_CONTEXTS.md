# Multi-source contexts for always-run and type-analysis

This implements resumable matching in the **standalone single-thread Cell CPU
runtime**, using the existing authenticated source -> SK -> marker -> typed
Recipe pipeline. It does not add a different parser, rewrite the source schedule,
or authorize terminal RipSat substitution. The current source library's custom
merge certification limitation remains unchanged.

The feature is opt-in and is **OFF by default**, because the final small Pass 3
measurement still regresses. Enable with `CELL_CPU_RETAINED_CONTEXTS=1`.
`CELL_CPU_RETAINED_SETS=always-run,type-analysis` is the default selected scope;
the comma-separated list can select other existing rulesets for tests. Only
multi-table queries use the new path. Single-table and empty-table shortcuts
remain on the existing inexpensive paths. No context/count/depth budget is used.

## Execution and invariants

1. A typed query owns a persistent tree of partial, correlated bindings. Existing
   source markers are preferred structural entries; otherwise the constructor
   binding the most equality coordinates supplies the entry. This is a static
   layout choice, not a learned optimal plan. It keeps the structural owner
   independent of whether a mutable value is ready. Remaining reads use the
   existing index/cardinality choice when the branch is first created.
2. Each branch stores its exact row values, FactRef or function witness, and
   publication epoch. Several branches can come from different producer Cells.
   Alternative values remain separate correlated branches; they are never mixed
   column-by-column. Missing per-key inputs retain an empty waiting domain.
3. The query index publishes both the **old and new indexed keys** of a changed
   witness after RHS, merge, union repair and reference relocation. These keys
   notify only subscribed read domains. A function value changing without its
   key changing still generates a notification. Fingerprint collisions cause
   extra notifications/candidate checks, never acceptance without exact checks.
4. A notified domain applies its changed row references to the previous domain.
   Insertions, replacements, hidden rows and deleted rows update that domain;
   unchanged branches remain. Parent links mark only dependent ancestors dirty.
   Parameter rebinding rebuilds the dependent subtree. Index resets trigger a
   full refresh; fixed equality literals are conservatively refreshed after
   union, including when no previously matched row contained the losing identity.
5. Internal nodes keep **factored branches, match counts and newest witness
   epochs**, rather than flattening complete answers at every ancestor. Only
   paths containing evidence new enough for the original rule watermark emit a
   binding. Naive rules still replay all current bindings on each allowed Run.
6. All emitted bindings are sorted/deduplicated by the original complete input
   coordinates. The existing executor seals the original Run's whole RHS patch
   before committing anything. No future stage receives an early effect, and no
   merge callback, RHS constructor, union or intermediate candidate is omitted.

The invariant is that a consumed domain is complete for its current partial
binding and visible graph. The first acquisition still searches the graph.
Subsequent updates reuse partial work rather than reusing a stale completed
answer. The ordinary source Run boundaries, guards, effects and repair remain.

The event subscriptions are also the invalidation mechanism: the implementation
does not keep historical bucket vectors alive as validity receipts. Normal index
clears and replacement invalidate contexts explicitly. It retains query slots
only, not unused RHS temporaries. Private context IDs use a reusable contiguous
pool; subscriptions are removed before reuse. These IDs are **not eclass IDs**,
and this pool assumes the executor's existing single-thread execution.

Removed/rebound subtrees are reclaimed when their scheduled query refreshes.
Pending changed witnesses can wait until that query runs; this is not an
asynchronous garbage collector. A checkpoint preserves the authoritative graph
and watermarks; query contexts rebuild after resume, without replaying old effects.

## What the counters mean

- `live_contexts` counts partial query-tree nodes, **not baked Cells**.
- `multi_producer_bindings` counts emitted bindings whose constructor witnesses
  have multiple FactRef owners. It is not a count of certified high-level Cells,
  and function witnesses are separately retained in the complete binding.
- `function_domains_refreshed` includes Unit marker functions, not just mutable
  numeric ports.
- `union_refreshes` counts reevaluations after the global effective-union count
  changed; it is not an independent causal attribution to union.
- Range hits, callback notifications and emitted logical bindings are not new
  committed facts. Constructors/unions still have their separate engine counters.
- Fixed port reads, inverse reads and candidate visits are reported separately;
  a moved read must not be credited twice.

## Final sum-bits Pass 3 measurement

Same release binary and same `out/local-sat-connected-compile` artifact. Merge
programs, existing multistage projections, copied marker fields and storage
takeover are ON. Local saturation, refutation, presearch answer caching, Sieve,
union-graft and local congruence shortcuts are OFF to isolate this mechanism.
Every configuration was timed once; audits/builds/tests did not overlap timing.

| Metric | Existing path | Retained contexts |
| --- | ---: | ---: |
| Inference | **128.133 ms** | 136.276 ms |
| Setup | 0.963 ms | 1.053 ms |
| Process wall, parsing/export included | **266.871 ms** | 283.778 ms |
| Peak RSS | 204.13 MiB | 211.55 MiB |
| Candidate row checks | 84,033 | **73,987** |
| Query row visits (`decoded_rows`) | 113,887 | **98,929** |
| Exact-key lookups | 62,212 | **57,633** |
| Fixed forward port reads | 29,795 | **23,146** |
| Fixed inverse reads | 3,762 | **2,880** |
| Constructor requests | 19,188 | 19,188 |
| Union requests / effective unions | 3,210 / 1,039 | 3,210 / 1,039 |
| Repair collisions | 951 | 951 |
| Run / repair calls | 1,496 | 1,496 |

The work reduction is real, but **net inference is 6.36% slower**. Do not claim
an end-to-end speedup. Per-stage search time is mixed:

| Stage | Existing | Retained |
| --- | ---: | ---: |
| always-run | 20.655 ms | 22.856 ms |
| type-analysis | 7.068 ms | **6.098 ms** |
| All search | 69.375 ms | 71.680 ms |
| Index/context publication | 4.629 ms | 8.501 ms |

Direct subscription publication accounts for 2.800 ms of the latter interval.
Other phase differences are observations from one paired run, not isolated
causal estimates. The first implementation cost 272 ms; targeted row deltas,
factored answer paths and pooled metadata brought it to 136 ms. All intermediate
measurements are retained, not averaged or selected as a best-of series.

Actual retained work:

- 216 selected query layouts, 99 with instantiated roots, 2,869 calls;
- 5,271 emitted bindings, 4,541 with multiple constructor FactRef owners;
- 4,643 incremental domain refreshes and 9,659 full/initial domain acquisitions;
- 22,146 row-to-context notifications, 6,954 context invalidations;
- 1,469 function/marker domain refreshes, 90,556 subtree reuses;
- 9,640 live prefix nodes, 9,046 branches, 6,526 subscriptions;
- 4,962 retired nodes, whose slots were reused; 9,640 context slots at the end;
- 87,699 stored input/branch value coordinates (logical slots, not RSS bytes).

The remaining bottleneck is maintaining and interpreting many small per-rule
contexts. Prefixes are retained within each query, but are not yet shared as one
execution node across different rules. A promising next step is to compile and
share those common structural prefixes and separate their invariant coordinates
from changing ports. Increasing the number of stored Cells is not evidence of
benefit. Cross-ruleset continuous RHS execution has deliberately not been enabled
to hide the current matching-maintenance cost.

## Correctness

72 Python tests pass (including existing 52-case suites inside test methods),
plus 7 Rust unit tests. Dedicated native-backed examples cover:

- separate producers and multiple correlated alternatives;
- absent inputs arriving later, numeric value replacement, and false/true guards;
- union creating new matches, parent congruence, and fixed global inputs;
- custom merge producing `Choose`, then a later ruleset performing the union
  that exposes a `Pair` to the waiting consumer;
- delete/subsume, forced fingerprint collisions, index resets and recycled IDs;
- finite Run visibility, naive effects, repeated updates and checkpoint restore.

Real Pass 3 checks compare every complete graph and every binding at **1,497
snapshots, including initialization**. All values, equalities, visibility flags,
Run paths/iterations and logical matches agree exactly. A fresh process reproduces
the exact output state. Final graph: 4,314 records / 2,324 eclasses. This comparison
is against the existing standalone engine on the same compiled input; the older
raw-native whole-Pass-3 discrepancy is not resolved by this change.

## Reproduce

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml \
  --bin eggfuse-cell-runtime --example project_stages --example project_semantics

EGGFUSE_TEST_FRONTEND=/tmp/eggfuse-ripen-build/debug/examples/eggfuse_frontend \
  /opt/homebrew/bin/python3 -B -m unittest \
  experiments.cell_runtime.retained_test experiments.cell_runtime.local_sat_test \
  experiments.cell_runtime.presearch_test experiments.cell_runtime.refutation_test \
  experiments.cell_runtime.refutation_cores_test experiments.cell_runtime.known_results_test

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --compiled out/local-sat-connected-compile --feature retained \
  --output out/retained-contexts-repeat --audit
```

The benchmark extends the existing timer and all-stage streaming auditor; it does
not rebuild a raw-source alternative pipeline. Compact receipts are stored in
`retained_context_results.json`; complete local outputs are under
`out/retained-contexts-{first,subscribed,pruned,factored,pooled,final}`.
