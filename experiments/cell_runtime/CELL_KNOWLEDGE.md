# Cell knowledge: metrics before optimization

2026-11-03. Standalone Cell CPU, fixed sum-bits Pass 3 source→SK→marker pipeline.
Merge programs now default ON (`CELL_CPU_MERGE_PROGRAMS=0` is the ablation).
No raytrace, GPU, solver selection or new source-marker optimization is involved.

This iteration implements and tests result knowledge, then tightens the union
deformation metric. It does **not** complete arbitrary boundary-delta Bake,
mutable multi-stage execution contexts, or a general A→B→C Cell upgrade runtime.

## What the instance knows

Instances now record their start epoch and executed RHS prefix alongside their
existing shared template, correlated SSA slots and owned facts. The prefix means
instructions executed/enqueued, not permission to expose a future Stage. Original
Run/Repeat/Saturate and sealed StagePatch remain authoritative.

The compiler exports `known-results.json`: exact full-key projections from a
matched query row or a previous effect to a later constructor operation. Rust
independently reconstructs these projections from the typed source. There are
225 such static operations in this workload (101 query-source, 124 effect-source).

The reference experiment retains weak FactRefs only at useful operation positions;
it does not pin the owner or keep empty application frames alive. Relocation is
resolved, existence and complete canonical key are checked, and dirty domains
fall back. Delete/recreate invalidates old references. Hidden-row construction
retains the ordinary engine's behavior. No mutable function set or union is skipped.

The instruction experiment uses a stronger lifetime condition: a row existed in
this Run's sealed body snapshot, or was constructed earlier in the same ordered
RHS; its table has not suffered a deletion in this Run, and its domain is repaired.
The compiler can then copy its existing SSA result. A per-table deletion epoch
revokes the shortcut; original lookup remains available. It does not need another
FactRef map or extra LHS witness collection. Ordered source execution, not a
heuristic estimate of Cell similarity, establishes instruction precedence.

## Step 1: shadow the opportunity first

`CELL_CPU_KNOWN_RESULTS=observe` still performs the original lookup and compares
the proposed reference/value. This is diagnostic and not timing-eligible.

| Quantity | Count |
|---|---:|
| Constructor operations entering this path | 19,199 |
| Operations returning an existing constructor | 15,966 |
| Exact known-result opportunities | 4,710 |
| From an already matched LHS | 2,559 |
| From earlier RHS operations | 2,151 |
| Different actual witness / different result value | 0 / 0 |
| Retained weak references at end | 1,838 in 438 frames |

These are **29.5% of duplicate constructor operations**, not 29.5% of all runtime
work. Dynamic-constructor lookup fast paths are a separate path; the 19,199
counter must not be presented as every possible constructor call in the engine.
Largest opportunities: TCPair 892, TermGet 882, Get 716, NoTerm 579,
TermSingle 492, Single 318, Concat 285. The counter measures real operations,
not template frequency or storage compression.

## Four implementations, separate single-run controls

Every lane keeps merge programs enabled; each candidate is compared with the
same binary and identical program. Builds and diagnostic observers are outside
the timed interval. These are successive implementations, not repeats from
which a favorable number was selected.

| Method | Control | Candidate | Change |
|---|---:|---:|---:|
| Retained FactRef + revalidation | 138.551 ms | 140.127 ms | +1.14% |
| SSA reuse dispatched inside constructor path | 140.509 ms | 143.615 ms | +2.21% |
| Reuse compiled into RHS instruction dispatch | 136.557 ms | 137.124 ms | +0.42% |
| Static constructor elimination + lazy slot aliases | 136.139 ms | 135.033 ms | −0.81% |

The first three avoid **4,710 keyed lookups**, 62,482→57,772. The fourth removes
**6,323 constructor operations** (19,199→12,876), with keyed lookups 62,482→56,159.
Candidate visits remain **86,393**. Only the static method has a small nominal
improvement in its single run; this is not evidence of a stable or large speedup.
They remain opt-in: `on` selects references, `value` selects instruction-time
copies, `static` selects constructor elimination, and `off` remains the default.

The first approach pays to collect witnesses, retain references and decode/check
rows. The second still searches program metadata per construction. The final
approach shares an immutable instruction projection per rule and directly copies
the bound result, eliminating that extra reference storage. An intermediate
per-instruction progress write was also replaced with a local cursor and one
final frame update; it had no reason to repeatedly look up the frame.

The static pass propagates exact constructor identities through SSA, exposing
nested repetitions the first 225 projections missed: **287 static aliases**.
For LHS facts it requires that the source program cannot delete that constructor
table. Within one sequential RHS, a delete clears that table's available facts.
Function reads, sets, guards, unions and visibility operations remain untouched.
This is a source-program/sequential-execution proof, not permission for an external
mutator or future parallel backend to delete rows between these instructions.
Original semantic templates remain unchanged; executable steps skip the proved
duplicates, and fact decoding follows the proved slot aliases. No additional
instance values or weak-reference map is required. Static checkpoints must resume
with the same representation; a different mode is explicitly rejected.
The additional shadow check agrees with ordinary results **6,323/6,323** times.

The available experiments establish that **avoided lookup count alone is not a
success metric**. Search was already paid before these RHS savings. Final
selection requires total time, work actually avoided, allocations/reference
storage, and maintenance costs together. Source-kind/witness counters are filled
by the observe/reference modes; instruction mode reports value reuse separately.

Every measured method passes 1,511 full snapshots and complete binding sequences.
Tests cover deletion, subsumption, union, fresh-process resume, forged projections,
default merge activation and all 52 existing semantic fixtures. A redundant typed
constructor instruction models marker lowering, because the native frontend CSEs
the equivalent tiny source expression before execution.

## Step 2: distinguish structural growth from relevant changes

The new observer uses **full canonical constructor keys**, not only operator
counts. Pre-union rows are normalized under the final UF before comparison, so
representative renaming and coalescing duplicate keys are not mistaken for new
structure. It also projects onto the original recipe's actual constructor reads
and inspects current function-key values. Historical bound function values are
checked separately: a retained fact frame is not necessarily a current execution
context. Markers are excluded from the mutable-function read comparison.

The observer remains read-only. It is attached to union events, so function-only
updates without union are **not** comprehensively counted. RHS read/merge-target
validity and all possible outside consumers are also not certified by this metric.
The before-snapshot is immediately before the first relevant union, after RHS
submission; it can contain unresolved merge results. Some stale readings become
current again after repair. Historical storage frames are not all active contexts.

There are 1,037 effective unions (525 Apply, 512 congruence). Among existing
completed original-rule frames, 3,141 frame×Run records have a direct Apply-union
input. All 3,141 gain some constructor membership; this is largely a natural
consequence of joining classes and is not by itself useful Cell credit.

| Measured input property in those 3,141 records | Count |
|---|---:|
| Constructor rows read by the current recipe change | 629 |
| Function-port domain changes during repair | 18 |
| Stored function reading already stale before the observed union | 254 |
| Stored function reading stale after repair | 224 |
| Slot alias relationships change | 190 |
| Current measured input projection unchanged | 2,124 (67.6%) |

Categories above overlap. In the larger set of 5,859 impacted existing frames,
4,584 have unchanged current input projections. These counts do not authorize
skipping arbitrary matches or effects.

Weighting by real work is essential. Only **146** of the 2,124 unchanged-input
records own repair work, totaling **218 visits**. The mutually exclusive work
partition (stale function snapshot takes precedence) is:

| State category | Attributed repair visits |
|---|---:|
| Unchanged current input projection | 218 |
| Changed input read projection | 131 |
| Slot alias change | 359 |
| Stale function snapshot | 96 |
| Total | 804 |

These are associated visits, not saved visits. The whole run has 2,378 repair
visits. A high unchanged-instance percentage does not imply equal time savings.

## Consequence for the next experiments

Three distinctions must guide boundary-delta Bake and incremental completion:

1. A new class member can be irrelevant to the currently selected recipe.
   Track projected read changes and actual downstream work, not total membership.
2. A frame's old bindings may no longer describe current function ports. Preserve
   the historical fact recipe separately from a mutable execution context; blindly
   overwriting its slots could alter existing virtual facts (e.g. replacing the
   old TCPair cost row instead of retaining it alongside a new candidate).
3. Finished constructor results are cheap to reuse but are too late to avoid the
   major search cost. Next measure event-to-context routing, newly read candidate
   domains, previously completed effects still valid, and downstream queries
   actually eliminated. Input stability alone is not effect completion: ordered
   merge callbacks and RHS reads need their own dependency checks.

The rare alias changes carry substantial attributed repair work. Before broad
expansion, evaluate alias/input-to-output transitions by work weight, including
whether current function dependencies allow local propagation. Do not blindly
enable the existing globally gated congruence shortcut.

## Reproduce and artifacts

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml --bin eggfuse-cell-runtime --example project_stages
python3 -B -m experiments.cell_runtime.known_results_bench --pipeline COMPILE_DIR --output out/NEW --observe
python3 -B -m experiments.cell_runtime.known_results_bench --compiled out/NEW/compile --output out/MEASURE --method value --audit
python3 -B -m experiments.cell_runtime.union_bake_survey --compiled out/NEW/compile --output out/SURVEY --audit
```

Compact results: `cell-knowledge-results/`. Raw measurements are under
`out/known-results-measured-sum-bits/`, `out/known-value-measured-sum-bits/`, and
`out/known-instruction-measured-sum-bits/`. Read projection observations are under
`out/cell-read-deformation-sum-bits/`; work-weighted aggregation reuses that exact
report and audit in `out/cell-read-work-sum-bits/`. No additional dataset run was
needed for that aggregation. The old frozen static Bake receipt is still reported
as such; no certified whole-Cell upgrade count is claimed.

The static experiment uses `out/known-static-compile/`,
`out/known-static-measured-sum-bits/`, and `out/known-static-observed-final/`.
Choose `--method static` after re-emitting the device pass. Its full artifact must
contain the checked static alias program. The auxiliary shadow/cold-validation
additions after the measured build do not supply another timing sample.
