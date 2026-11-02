# Union causes -> implicit Spine entries -> scheduled RHS

Implemented in the standalone single-thread CPU engine, behind
`CELL_CPU_UNION_GRAFT=1` (default off). `CELL_CPU_UNION_TRACE=1` records only
causes. No native egglog or GPU execution path is modified.

## Executable path

The analyzer's existing multi-stage pass emits `union_entries` from the typed
Recipe IR. An entry describes a rule with exactly two constructor/relation atoms
(constructor tables in this IR) sharing an equality-valued variable. The common
variable can occur in an argument or result column: non-root input refinement
is covered. The original primitive guards and RHS are retained. The Rust loader
independently reconstructs and checks this wiring; forged projections fail.
Unsupported queries, naive rules and probe queries keep ordinary matching.

For example:

```
Holder(x), where x was opaque
union(x, Bop(a,b))
    -> repair publishes a complete neighborhood for x's equality class
    -> the scheduled consumer receives Holder(x) + Bop(a,b) directly
    -> evaluate guards, enqueue its complete binding into the normal StagePatch
    -> execute RHS only at the original Run's publication point
```

This path has no explicit marker row. It replaces that seed's general matching
path, rather than executing a second copy alongside it. The neighborhood retains
all visible alternatives; repeated-variable/literal constraints and the original
semi-naive latest-witness partition still apply. The next stage cannot observe
these effects early. Non-union insertions and unsupported structures fall back.

After repair and storage takeover, neighborhoods are gathered from the existing
equality reverse-dependency index. Constructor/relation table versions guard
their completeness, including empty domains. Insertion, deletion, subsume and
further union invalidate the corresponding domains. Table-to-context subscriptions
avoid rescanning every context on every Run; displacements remove obsolete root
contexts. This is still conservative table-level invalidation within each class
context, not a minimal per-key update scheme. Snapshots currently copy rows;
their construction and maintenance are charged to the feature.

## Cause graph, not rebuild loop counts

Every effective union records requested endpoints, actual canonical endpoints,
Run epoch, and its source. A direct apply records rule/frame/op identifiers; a
constructor congruence records the two fact references and the argument-equality
causes. A frame ID is a historical application identifier, not a promise to keep
its entire binding alive after garbage collection.

Representative canonicalization dependencies are retained too. For example,
`a=b; b=c` cannot attribute the derived `a=c` equality solely to the second apply.
UF parent edges reference shared cause records, avoiding copies of the entire
ancestor history. Only dirty pre-repair keys are temporarily copied for argument
explanations; they are released after repair. Already-equal union requests do not
become new effective-union events.

Depth is the height of the selected congruence proof, with union transitivity
not adding a constructor layer. Canonicalization support contributes its existing
height. It is not a shortest path, syntactic/physical distance or rebuild counter.
Container-key explanations and local batch-closure provenance that are not
implemented are explicitly unknown (`depth=null`), never guessed. Checkpoints
retain and validate cause DAG order/parent links; runtime neighborhood caches can
be discarded safely and rebuilt by later events or ordinary matching.

## sum-bits Pass 3

The official source -> SK -> marker artifacts are reused. Only the CPU device
pass is re-emitted, exporting 307 typed slot-entry plans (604 distinct rule/seed
dispatch keys). No new Bake or solver execution. The same release binary runs
each lane once, with all existing source markers, multistage execution, copied
fields and storage takeover fixed; refutation, Sieve and the separate observed
composition experiment are off.

| Lane | Inference | Peak RSS |
|---|---:|---:|
| Feature off | 235.371 ms | 207.75 MiB |
| Cause recording only | 239.526 ms | 208.02 MiB |
| Cause recording + graft routing | 258.084 ms | 211.22 MiB |

The combined feature is **9.65% slower** and remains disabled. Search time is
122.519 -> 122.612 ms: query work decreases but not search wall time. The update
phase grows from 8.352 to 28.144 ms, including neighborhood preparation and
retirement. An earlier implementation scanned all neighborhoods per Run and
regressed 249.411 -> 349.776 ms; table subscriptions removed much of that cost.
Those are different implementations, not repeated trials selected for a best time.

The route provides **1,977 complete bindings**: 1,652 for original eggcc rules
and 325 for helpers. They are not necessarily new facts or useful committed
effects. Original consumer distribution:

| Ruleset | Routed bindings |
|---|---:|
| always-run | 975 |
| context | 458 |
| type-analysis | 142 |
| type-helpers | 42 |
| is-resolved | 35 |

Helpers account for 282 `cell-spine-complete` and 43 `cell-stable-keys` bindings.
8,954 seeds take the route, including 5,915 empty-domain cases. Empty rejection
is generic index credit, not a baked Cell proof. There are 4,049 supplied row
pairs and 6,729 preparation row reads; these are reported alongside the ordinary
candidate reduction 151,874 -> 149,348. Key lookups fall 97,113 -> 96,061;
conditional inverse reads fall 8,815 -> 8,135. None of these counts alone is an
end-to-end speedup.

All **1,089 effective unions** have identified causes here: 576 apply-origin and
513 constructor-congruence events. Three events also require representative
canonicalization parents. Selected proof-depth histogram:

| Depth | Events |
|---|---:|
| 0 | 576 |
| 1 | 283 |
| 2 | 161 |
| 3 | 40 |
| 4 | 15 |
| 5 | 10 |
| 6 | 4 |

93.7% are at depth <=2. This supports shallow causal propagation on this input;
it does not prove small fan-out, compact Cell boundaries, or a shallow AST.

## Validation and limits

- 1,622 streamed snapshots and full binding sequences agree off/on, including
  all helper tables, equalities, function values, visibility and Run coordinates.
- Fourteen integration tests pass, one covering all 52 existing semantic fixtures.
  Targeted examples also compare against the native frontend's actual execution.
- Tests cover non-root grafts without markers, finite-stage visibility, multiple
  alternatives, invalidation, guards/repeated bindings, no-union fallback,
  two-level congruence, multiple causes, transitive canonicalization and resume.
- Python/Rust reject forged union-entry wiring. Legacy artifacts remain usable
  with the feature off; enabling graft on an old artifact explicitly requires
  re-emitting the CPU device pass. There is no silently empty activation.

This is an event-driven entry into existing rule recipes, **not automatic
replacement of an arbitrary Cell by a new certified RipSat terminal state**.
Complex queries and complete alias-specialization families are not covered.
The native original-source baseline and pre-existing Cell/native graph differences
remain as documented in `SOURCE_NATIVE_BASELINE.md`; no new speedup or full Pass-3
equivalence to pristine egglog is claimed.

## Reproduce

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml --bin eggfuse-cell-runtime --example project_stages
EGGFUSE_TEST_FRONTEND=/path/to/eggfuse_frontend python3 -B -m unittest experiments.cell_runtime.union_graft_test -v
python3 -B -m experiments.cell_runtime.union_graft_bench --pipeline out/marker-shape-sum-bits/remove-compile --output out/NEW --audit
```

Subsequent engine-only runs use `--compiled out/NEW/compile` to reuse validated
compiler output. The script reuses the common pipeline loader, timer, metrics
and streaming audit. Initialization, compile/Bake, export and diagnostics are
outside inference. Raw final results: `out/union-graft-sum-bits-attributed/`.
Compact evidence: `union-graft-results/sum-bits.json`. No raytrace workload was run.
