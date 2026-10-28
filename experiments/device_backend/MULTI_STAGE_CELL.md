# Baked multi-stage Cell and immutable stage context patches

This is an opt-in path in the actual Metal-owned backend. It uses the certified
`terms`, `terms-helpers`, `terms-helpers-helpers` program and the original nested
schedule. Ridge is not part of this path. It is not a general discovery algorithm
for arbitrary multi-stage Cells or a replacement for all eggcc passes.

## Bake contract

`--bake-stage-cell PATH` runs the program with stage patch auditing and saves a
loadable contract after successful completion. The package contains:

- The exact nested schedule: Run terms, then the nested helper/comparison loops.
- A separate matching Spine for every rule in each stage: typed slots, table
  atoms, shared-variable wiring and primitive guards. Different bindings remain
  distinct; a root does not identify one unique binding.
- Stage read/write tables, including constructors introduced by function merge.
- The equality footprint and the versioned patch component layout.
- Relative source Spine interfaces and rule-derived cost programs.
- Observed scope/patch counts and explicit native-check provenance.

Concrete input facts, global `let` names and generated assertion tables are
excluded from the template key. Loading validates the whole contract against
the current typed rule environment. A different input can instantiate the same
package. It does not replay the Bake sample's final graph, and the package does
not override finite Run scheduling. Source parsing/typechecking remains native.

## Runtime versions

```text
immutable input C0
    -- terms --> patch P0
C1 = apply(P0, C0)
    -- helpers --> patch P1
C2 = apply(P1, C1)
    -- saturate comparison --> patch P2
C3 = apply(P2, C2)
    -- follow the original nested-loop schedule --> ...
```

The matcher reads immutable versions through a page map. Unchanged 1024-byte
row pages share storage; changed pages get new physical storage. The page map
keeps logical row identities stable, including non-root witnesses. Matching and
comparison kernels read this representation directly, without flattening it.
The pool grows from actual changed-page demand and returns pages on the GPU
only after the last version reader has released them. Growth preserves existing
physical indices and all old views. Debug exports flatten pages for inspection.

Each view versions rows, union-find, fact redirects, rule watermarks, metadata,
function-map coordinates and per-table read lists. All are tied to one version.
Source marker interfaces and the immutable container arena are shared within
one invocation. Container content immutability follows the existing schema
certificate and is checked in audit mode; the arena's normalization epoch is
a cache tag, while stage visibility uses the frozen view's epoch.

Views use the same handle namespace, but resolve equality with **their own**
parent array. Later union, function updates and retirement cannot change an
older view's answer. The read path uses read-only pointers; effects still go
to the joint program's separate transactional working state.

The logical patch remains exact u32 replacement bits, preserving i64, f64,
visibility and timestamps. A row publication journal is written by actual
constructor/function changes, canonicalization and retirement. GPU dispatch
first selects published pages, then compares and copies only those pages.
Sharing an untouched page uses one thread, not an entire copying threadgroup.
New row extents are always included, even when their bits are zero. Audit still
compares every reconstructed row word, independently of the journal.

The smaller auxiliary columns remain flat. Their patches are counted and
applied in one GPU pass; offset/value records are compacted only for debug
export. Certified append-only read lists share storage when their extents do
not change. The function-key map is reusable when no new function row appeared,
because its Expr keys cannot change inside this certified scope. Other skips
require a no-action/no-comparison-change condition. Audit checks shared columns
against live state too. This is not arbitrary mutation journaling for all sorts.

The complete correlated TCPair witness projection is shared across stages. An
exact publication generation invalidates it on insert, changed canonical ID,
visibility or timestamp. New IDs without a cost witness extend it by zeros;
its previous buffers are preserved when they grow. It never reduces multiple
candidate witnesses to independent marginals or a single winner.

Row-patch discovery runs in the effect command buffer before its existing status
read. The successor seal is queued without waiting on the CPU. The next stage
reads it in the same ordered Metal queue; errors and telemetry are consumed at
that stage's normal status boundary. Debug export can wait to read data. This
removes the **extra seal waits**, not every existing host synchronization in the
joint program. The original schedule and dependent effect order are unchanged.

The live set is the invocation entry, the latest input for each of three stages,
and the next materialized input. It does not retain every iteration's full graph.
Submitted Metal commands retain their buffers; old storage is not overwritten
while a reader holds it. Published graph compaction remains at the joint exit;
the next invocation reacquires its source interfaces.

## Parallel reads and boundaries

One two-dimensional GPU scan reads the entry and three stage views together.
Each threadgroup belongs to one context. Its table selector comes from that
stage's baked read contract, rather than a mixture of rows tagged with stages.
Debug probes resolve equality and unary function reads in the selected version.
A stale version token and a patch applied to the wrong base are rejected.

This verifies simultaneous **read-only scanning** of independent versions.
Dependent stage effects still follow the original schedule. Arbitrarily merging
two patches derived from the same base is not implemented: the second patch
cannot silently be applied on top of the first one's result.

Current limitations: the working row store is still materialized and RHS commit
is ordered. Auxiliary state still uses exact flat-column snapshots, rather than
a general incremental column engine. Projection arrays are indexed by the active
identity domain; this is not yet a fully compact per-Cell parameter-matrix ABI.
The matching program requires the certified terms ABI and an immutable Expr
carrier during each joint invocation. Capacity/status waits in the existing
executor remain. No claim of arbitrary concurrent ruleset effects is made.

## Run

```sh
python3 experiments/device_backend/build.py --worker-only \
  --reuse-native-build out/device-build-feature-refresh-final \
  --output out/stage-build --target target/device-backend

# Reproducible small native/GPU examples, including a Bake file and new input.
MTL_DEBUG_LAYER=1 EGGFUSE_MATRIX_AUDIT=1 \
python3 experiments/device_backend/test_stage_cell.py \
  --build out/stage-build --output out/stage-tests

# Bake a real program. The original source still supplies facts and scheduling.
python3 experiments/device_backend/run.py --source /path/to/input.egg \
  --build out/stage-build --output out/stage-bake \
  --bake-stage-cell out/library/terms.cell.json

# New process / new input, same typed stage environment.
python3 experiments/device_backend/run.py --source /path/to/new-input.egg \
  --build out/stage-build --output out/stage-use \
  --stage-cell out/library/terms.cell.json
```

`--stage-contexts` derives the contract directly without saving/loading a library.
`--audit-stage-contexts` checks each reconstructed component against the actual
post-stage state, before continuing. `--export-stage-contexts` exports the entry,
latest stage views, latest patches and GPU probe answers for small diagnostics;
it does not export every historical view. Both modes are excluded from clean
timings. Normal execution downloads control/count information, not bindings or
table contents for CPU execution.

`stage-contexts.json` records every version edge and changed-word counts, plus
copy bytes, retained storage, GPU maintenance time and parallel scan results.
The tests reconstruct exported patches independently, inspect old/new equality
and function answers, load the same library with new bindings, reject changed
contracts and stale handles, preserve finite Runs, restore checkpoints, and
check overflow/panic rollback.

## Original implementation: validation and cost, 2026-10-28

[Checks](multi-stage-checks.json): 12 new multi-stage checks plus 115 related
backend/joint/comparison checks passed. The real initial Raytrace terms scope
also passes the independent native comparison at **27,793 rows / 21,655
eclasses**. That run produces a reusable package; a fresh process loads it for
the complete saved Raytrace Pass-3 source and schedule.

The complete run accepts **12 joint invocations** and produces **2,826 stage
patches**. A separate audit checks each reconstructed component against the live
state and rejects deliberately wrong-base applications. Its step records agree
with the clean run and control. Final complete typed facts, values, visibility
and equality partitions agree with the previous GPU engine at **699,784 rows /
524,742 eclasses**. This does not repair that engine's pre-existing complete-Pass
native difference. Ordinary Run was not turned into Saturate.

This first representation is **not a speedup**:

| Full Pass 3, no ridge | Inference | GPU commands |
|---|---:|---:|
| Joint program without stage contexts | 40.519 s | 29.277 s |
| Loaded multi-stage Cell with stage contexts | 53.800 s | 38.166 s |

Each is a single clean run; setup, graph checks, audits and debug exports are
excluded. The control was retained while changing only the enabled stage-patch
encoder. [Results and fingerprints](multi-stage-results.json) identify both builds.

The stage path records 16,557,297 changed words (offset/value records about
132 MB), but constructing contiguous immutable views copies **161.03 GB** over
the complete run. Its stage-maintenance command time is **7.27 s**, with peak
tracked context storage **753,389,284 bytes** (about 718.5 MiB, not total RSS).
Whole-component materialization amplifies the small patches substantially.
That storage/dispatch cost is exposed rather than attributed to Cell reuse.
That implementation remained opt-in; the optimization below replaces its storage
and maintenance path. Dependent-stage execution remains ordered.

## Shared pages, publication journal, projections and asynchronous seals

The same saved Raytrace Pass-3 source and complete schedule were used. These are
single clean timings, with no fresh pristine CPU baseline or hardware occupancy
measurement. [Results and fingerprints](stage-optimization-results.json) retain
the full-graph check and actual stage counters.

| Configuration | Inference | Context materialization | Tracked context peak |
|---|---:|---:|---:|
| Historical joint engine, contexts off | 40.519 s | — | — |
| Original versioned contexts | 53.800 s | 161.03 GB | 753.39 MB |
| Shared pages + journal + projection reuse + queued seal | 39.798 s | 12.79 GB | 235.47 MB |

This removes 26.0% of the regressed inference time. The final single run is
1.78% faster than the saved contexts-off baseline; that small difference alone
does **not** establish a stable speedup. The main demonstrated gain is removing
the 32.8% regression while preserving independently readable stage versions.
Context materialization includes page-pool growth and flat auxiliary writes,
not just blit commands; it excludes the pre-existing transactional working copy.
Tracked context peak is not process RSS or total GPU allocation.

Across 2,826 phases, the cost projection is rebuilt 454 times and reused 2,372
times. Extra host waits for patch count/application are zero in the production
path; initial capture, final scan and existing inference status waits remain.
Dedicated stage GPU time excludes publication-journal work fused into existing
commit commands, so use complete joint/whole-run time for performance comparison.

Intermediate variants are retained, including an over-reserved page pool
(43.561 s, 2027.74 MB tracked context peak) and a journal version (46.126 s).
An intermediate sparse-dispatch version measured 40.735 s; the final version
also preserves parent-array versions when path compression might change them
without a new union. These variants motivated demand-driven allocation,
eliminating unused diagnostic compaction, and dispatching copying threads only
for actually changed pages.
Do not select the best intermediate time and present it as the final code.

Reproduce using the earlier Run commands with the current build. The Bake file
contract is unchanged: concrete page placement is not part of Cell semantics.

Validation: [127 relevant checks](stage-optimization-checks.json), including 14
stage tests with old-version probes, correlated alternatives, finite Run,
external union/subsume, dynamic page growth, no-export asynchronous execution,
overflow/panic rollback, plus 60 backend, 19 comparison and 34 joint-program
checks. The final build also passes all 2,826 full-Pass patch audits and preserves
the exact internal phase history. Final complete typed graph equals the control
at 699,784 rows / 524,742 eclasses. Independent native initial terms comparison
passes at 27,793 rows / 21,655 eclasses. The manifest records the suite builds;
the last change only tightened parent-array sharing in the stage path.


## Deferred block publication

The next step is [block effects](BLOCK_EFFECTS.md). When a stage Cell is loaded,
helper function updates can remain pending through the comparison stage, and
equality is returned as a compact edge block. The merger joins those edges with
live equality and publishes the prepared function values before the next
consumer iteration. `--eager-stage-merge` selects the earlier publication path.
The historical timings and checks above belong to the earlier storage change;
see the block-effect report for its separate validation and performance.
