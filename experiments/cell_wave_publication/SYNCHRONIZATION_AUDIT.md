# Resident Cell publication: synchronization and serialization audit

Date: 2026-10-30. This audits the **actual ABI7 resident GPU engine** used for the 185.707 s Raytrace Pass 3 run, not the older hybrid backend or a standalone microbenchmark. It is a source audit plus saved-run counters, not a new timing profile. Priorities below are architectural hypotheses, not measured time shares.

## Reproduction boundary

- Repository HEAD at inspection: `0f1fd44`, with the current uncommitted Cell-wave implementation.
- Measured worker SHA256: `19a4d18f014505d62dc20de750245c1c321fb058aa3f2628bf64c0f10466a305`.
- Executed input: `/Volumes/Extreme SSD/eggfuse-profile/cell-waves-pass3-final/pass3-on/program-0/attempt-0/program.json`.
- Result: same directory, `result.json`; aggregate: `/Volumes/Extreme SSD/eggfuse-profile/cell-waves-pass3-final/results.json`.
- One global graph arena, `quantum=8`; all 30 declared ruleset query switches support parallel anchors. An arena is the complete resident graph, not a Cell.
- `marker_program=null`, `region_program.marker_names=[]`, `spine_cache_execution.enabled=false`. The recipe/context path is enabled, but this performance input does **not** use the newly generated source marker tables. Do not call this a benchmark of the complete source-marker pipeline.
- Enabled: parallel matching, match ordering, resident query/hashcons index construction, compact seeds, resident constructor writes, Cell waves, parallel union, resident rebuild and a certified private schedule region.
- 257 lowered templates; 204 admitted by Cell waves. This is not 204/257 runtime coverage.
- 36 private schedule invocations / 782 logical private Runs; one static private program. Ordinary and private logical Runs total 9,784.
- Inference: 185.707 s. Accumulated reported GPU command-buffer durations: 182.760 s; neither number proves high GPU occupancy. Shader/pipeline preparation: 293.018 s, outside inference.
- 2,971 **outer** command-buffer waits. Private schedule waits are additional and absent from this counter.
- 9,971 Cell waves; 1,422 parallel union batches; 1,502,316 wave requests; 211,617 wave fresh rows; 1,348,702 key groups; 33,950 duplicate-key requests.
- 3,566,974,816 seed row visits / 4,168,730 compacted active seeds; 1,215,004 keyed successor guard reads.
- 723,208 residual invocations, not all scalar: these also include the admitted resident constructor-batch path. Direct single-step scalar-path invocations: 91,253. Custom merge requests recorded by the Cell publisher: 125,340; this is not a complete count of rebuild callbacks.
- 1,460 hashcons index rebuilds is a combined counter: it does not distinguish parallel from scalar rebuilds.
- Two checked full executions passed per-Run fingerprints/control and final GPU state repeatability. The final graph differs from the disabled historical control in equality partition despite both having 701,998 rows; the cause is not diagnosed. Do not describe this as CPU endpoint equality.

## Execution path

```text
CPU submits a fixed quantum
  GPU single-arena control: choose the current Run
  prepare / update indexes
  compact active anchors
  one GPU lane per anchor, nested query loops within that lane
  complete binding materialization and deterministic ordering
  one lane builds all tile descriptors
  parallel context arithmetic / exact checks
  publisher cursor chooses a contiguous eligible segment
    eligible:
      propose one pending global effect per context
      group keys -> reserve identities -> publish -> index -> reply
      repeat until this segment cannot advance
    residual:
      ordered scalar effects, or admitted constructor batches
  drain pending unions -> flatten active parents
  rebuild -> ordered duplicate callbacks -> possibly rebuild again
  compact rows -> invalidate indexes
  resume blocked contexts or finish the Run
  repeat according to compiled Repeat / Saturate control
CPU waits, checks status, grows scratch if necessary, submits another quantum
```

The private fixed-carrier program takes a separate path: count/gather marker coordinates, allocate private state, execute several original stages, sort and publish output, then return to resident control. It has its own CPU waits.

## Full inventory

Status meanings: **observed** = enabled and supported by run counters; **reachable** = active generated code can take this path but no isolated count/time exists; **inactive** = excluded from this run. A serial path's presence does not establish that it dominates runtime.

| ID | Where synchronization / serialization occurs | Status and source | Change direction / dependency to preserve |
|---|---|---|---|
| S01 | The host waits for every outer command buffer, reads status and only then submits the next quantum. No overlap with preparing the following quantum. | Observed: 2,971 outer waits. `engine/metal/generic_cell.swift:248`, `:351`, `:471`. | GPU-generated dispatch and reusable command sequences; reduce host polling. Queueing farther ahead requires a safe growth/stop protocol, not simply removing the wait while CPU may replace buffers. |
| S02 | Private schedule entry counts coordinates, reads counts to allocate buffers, initializes/maps them, polls every eight stages, reads history and waits for final publication. Resident execution is stopped during this detour. | Observed: 36 regions / 782 private Runs. `engine/metal/scheduled_matrix.swift:38`, `:44`, `:51`, `:54`, `:55`, `:66`, `:72`, `:87`; adapted by `eggfuse/engine/scheduled.py`. | Reusable persistent coordinate/state arenas; GPU counts and indirect dispatch; keep trace on device. Region input/output visibility remains explicit. These waits are **not** included in 2,971. |
| S03 | Dynamic scratch growth requires a completed buffer, host allocation/zeroing/copying, pointer replacement and resume. | Observed: context growth 11, compaction capacity growth 13, write capacity growth 9; marker cache growth 0. The compaction count counts capacity doublings, not necessarily separate waits. `generic_cell.swift:286-328`. | Paged/segmented GPU arenas and reusable high-water capacity; grow only exhausted segments. Preserve pending contexts, effects and completion tickets. |
| S04 | The host encodes a fixed pipeline with a broad buffer memory barrier after virtually each phase: indexes, seeds, matching, sorting, evaluation, waves, union, writes, rebuild. | Observed enabled route: `generic_cell.swift:351-470`. Waves have 14 phases, union 4, residual writes 12, rebuild 15. Indirect zero-sized phases need not execute useful work. | Compile resource/effect dependencies; fuse local producers/consumers and narrow necessary barriers. Phase counts are **not** measured active dispatch counts. Do not remove write-to-read barriers without replacing the dependency. |
| S05 | One global graph control cursor advances the Run / Repeat / Saturate stack. Publication and rebuild gate the next ordinary Run and blocked continuation. | Observed: one arena. `batched_cell.metal:55-115`, `:251-269`; `scheduled_matrix.metal:14-24`. | Keep scalar schedule decisions, move independent dependency regions through device queues. A cheap scalar PC is not itself the main target; attaching whole-graph work to every PC is. Finite Repeat and Run snapshot semantics remain meaningful. |
| S06 | Matching is parallel across seed rows, but one lane performs the nested index-chain searches for a seed. It also visits relevant rules in a generated ruleset switch sequentially. Long branches hold up phase completion. | Observed code shape; per-seed tail distribution unmeasured. `compiler/targets/metal/compiler.py:212-314`, `generic_cell.py:149-160`, `batched_cell.metal:117-132`. | Compile marker-backed bounded context tiles; split genuine long fanouts into queued continuations and batch shared conditions. Avoid storing every Cartesian intermediate. |
| S07 | Accepted matches contend on one `m.matched` counter; wave statistics, changed flags, journal lengths and touched-index lists also use shared atomics. | Reachable and active atomics; contention time unmeasured. `compiler.py:295`, `cell_waves.py:48`, `cell_waves.metal:35`, `:54`, `:109`. | Per-block counts plus prefix allocation; block-reduced statistics. Required state publication must remain exact; instrumentation counters need not contend per effect. |
| S08 | All bindings wait for query completion, then pass through deterministic total ordering. Large batches use local bitonic sort plus multiple merge passes; small batches use threadgroup barriers. | Observed configuration: `batch_sort.enabled=true`. `batched_cell.metal:133-205`, `generic_cell.swift:400-416`. | Retain deterministic order only for genuinely conflicting, order-sensitive effects; let independent effects use stable logical identities and grouped placement. This needs explicit effect-dependence analysis. |
| S09 | After parallel sorting, **one lane scans all bindings** and emits every tile descriptor / selected plan / certificate bound. | Active serial code: `batched_cell.metal:196-215`. 46,371 tiles in this run. | Boundary detection, per-segment counts and parallel scan; compile rule-to-plan dispatch once. Do not require one lane to walk all 187k bindings in a large Run. |
| S10 | Every active query launch scans the global row pool to compact eligible seeds; a single lane totals block counts. | Observed: 3.567B visits for 4.169M seeds. `resident_seeds.py:26-54`. | Producer-published per-table/per-version row ranges or marker queues; parallel block scans. Preserve newly enabled seeds from union/update/subsume rather than assuming append-only rows. |
| S11 | Query indexes reset through several globally ordered phases; a reset uses six radix passes. Incremental append and changed-value patching still run serially from `bc_prepare_queries`; ordered bucket insertion may walk a chain. | Active code: `batched_cell.metal:15-27`, `query_index.metal:31-81`, `generic_cell.swift:365-391`; hierarchical radix upper offsets use 16 lanes. | Batch delta records by bucket, append immutable index runs and compact only affected portions. Stable query traversal is currently coupled to stable binding order; decouple only with an explicit ordering contract. |
| S12 | **Hidden full serial hashcons index rebuild:** `row_lookup` calls `ensure_row_index`. If invalid, the caller clears used buckets and inserts every live row in one thread. It can be invoked during scalar publication or a custom merge callback. | Confirmed in the actual compiled shader, not just unused source. Trigger frequency unisolated; 1,460 combined scalar+parallel rebuilds. `row_index.metal:14-25`, `batched_cell.metal:289`, `:298`; `resident_rebuild.py:37-45`. | Make invalid-index handling an explicit suspended request and resume after shared index work, or provide an exact base+delta lookup. A lookup must not silently perform a whole-graph rebuild. Never query a stale index. |
| S13 | Residual publication scans tiles and bindings in one lane, executing complete action programs. Eligible old constructor batches require at least 256 contiguous bindings; a single eligible wave context also remains scalar. | Active: `batched_cell.metal:271-305`. 91,253 direct single-step scalar invocations; 723,208 residual invocations include parallel constructor batches. | Preserve the required order only within actual conflict components. Mixed groups need per-context classification and batched proposals, not one ineligible member forcing unrelated work through a scalar path. |
| S14 | **One real effect per context per wave.** Copies/no-ops can advance locally, but a new constructor/set/union request returns to the global pipeline. Successor validation also reads actual tables instead of consuming an entirely local proved fact overlay. | Observed: 9,971 waves, 1.502M requests, 1.215M keyed guard reads. `cell_waves.py:60-127`, `cell_waves.metal:49-54`, `:111-121`. | Use local symbolic result IDs and an effect DAG so several recipe operations execute before global publication. Resolve identity-dependent guards at the necessary boundary; keep unvalidated facts private. Existing rows/functions need a consistent local overlay. |
| S15 | Key grouping uses CAS/probing; all members of one key form a linked list. One key owner serially traverses the list for min/max and member counts; hot roots also contend during union. | Observed grouping: 1.349M key groups / 33,950 duplicate requests. `cell_waves.metal:56-66`, `:89-104`, `:168-183`. | Grouped segmented reductions for associative effects; stable serial order only for noncommuting callbacks. Different keys are not automatically independent if a callback reads/writes other keys. |
| S16 | Several prefix-sum upper levels, reservation totals and certificate offsets are single-lane loops; every one waits for all producers. | Active: `cell_waves.metal:80-87`, `:125-129`; seed/query/merge/compact offsets; `batch_rhs.metal:52`, `:84`. | Hierarchical scans and one reservation per block. Keep a tiny final allocator if inexpensive; do not replace every scalar total with many tiny kernels by default. |
| S17 | After union requests, the engine flattens **all active parent IDs**, then gates continuation on rebuild. Every successful parallel union advances a conservative global merge generation. | Observed: 1,422 batches / 172,953 pairs. `cell_waves.metal:162-191`. | Accumulate safe local equalities; flatten or canonicalize only demanded/affected identities, and export touched dependencies. Alias-sensitive guards still require a canonical or exact local-union view. |
| S18 | Rebuild canonicalizes global rows, clears/groups all keys, collects duplicates, compacts rows and invalidates indexes. New unions/rows from callbacks cause another whole rebuild iteration. | Active: `rebuild_parallel.metal:31-127`, `resident_rebuild.py:57-91`. | Dirty-parent/use lists and affected-key worklists; stable fact handles reduce physical-row compaction. Incremental congruence propagation must include newly conflicting function keys and downstream readers. |
| S19 | **All duplicate-row merge callbacks are executed by one lane**, even for independent keys. A callback may construct more nodes, invoke S12, union results and restart rebuild. | Actual generated `resident_rebuild_merge` has `if(lane)return` plus the duplicate loop. `rebuild_parallel.metal:82-100`, adapted by `resident_rebuild.py:37-45`. Publisher custom requests 125,340 do not count all these callbacks. | Compile merge effects into proposals, parallelize proven-independent groups, reduce pure min/max separately and retain local ordered callback chains. This is a primary target for a focused profile. |
| S20 | Container-set canonicalization and dedup run serially; element sorting uses insertion sort and equivalence checks scan earlier set records. | **Inactive data path in this benchmark:** final set arena has 2 header words / 0 records. `sets.metal:4-35`, `rebuild_parallel.metal:33`, `query_parallel.metal:103`. | Needed for general programs: segmented canonicalization/sort/dedup. It cannot explain this Raytrace timing. |
| S21 | Completion certificate hash tables are reindexed serially at initialization/growth; wave certificate offsets are also serial. Cached proof checks still probe a hash table. | Local certificates enabled. `batches.py:44-74`, `cell_waves.metal:125-147`. With unchanged capacity, local reindex returns early: not a full rescan every Run. | Fold completion/version state into persistent marker records or parallelize rare reindex; preserve exact row revisions and visibility. Lower priority unless measured hot. |
| S22 | Threadgroup barriers coordinate scans, bitonic sorting, matrix loads/results and reductions. Some final reductions use one lane. | Active building blocks: `batched_cell.metal:142-195`, `:246-249`, `cell_waves.metal:75-78`, `scheduled_matrix.metal:68-72`. | Use SIMD scans and fused per-tile operations where legal. These local barriers are not CPU waits and should not be confused with whole-graph synchronization. |
| S23 | Audit mode performs per-Run fingerprint reductions and extra exact constructor/numeric checks; capture splits encoder scopes. | **Inactive in the 185.71 s timing:** `batch_audit_numeric=false`, `profile_capture=false`. `batched_cell.metal:325`, `generic_cell.swift:357-360`. | Keep diagnostics separately timed and optional. Do not explain production inference time using the audited reruns. |
| S24 | Shader compilation, checkpoint writing, final graph/JSON export and later graph comparison are sequential host work. | **Outside `inference_seconds`**; checkpoint time is zero here. `generic_cell.swift:44-138`, `:478-515`. | Optimize turnaround independently. They do not explain 185.71 s. |

## What must remain ordered

1. A consumer may not read an uninitialized result or a mixture of bindings. Local IDs/overlays can defer global allocation, not delete true data dependencies.
2. Two noncommuting updates that interact through keys, callback reads or emitted effects need a deterministic order. Pure associative reductions can instead be grouped. Merely having different output keys is insufficient proof of independence.
3. Ordinary Run snapshots and finite Repeat retain their compiled visibility/control contract. Proved private regions may run multiple stages without publishing globally; original control does not require a CPU round trip per stage.
4. A new union must eventually propagate through affected keys, function conflicts and query inputs. It need not force full-table maintenance immediately after every local operation.
5. Publishing a block requires identity resolution, exact guard/effect justification and correct visibility. The current GPU repeatability check is not a substitute for these obligations.

## Work order

The order is based on scope and architectural risk, **not yet on attributed GPU seconds**.

1. **Make S12 visible and remove hidden whole-graph work from lookups.** Count scalar index refreshes/rows inside publication and rebuild callbacks separately from parallel refresh. Make suspend/refresh/resume explicit before further wave scheduling changes.
2. **Remove large single-lane loops (S09, S13, S19).** Parallel descriptor construction; effect/merge dependence classification; only actual conflict groups ordered. Distinguish callback traffic from publisher traffic.
3. **Increase useful work per context publication (S14), then narrow union/rebuild work (S17–S18).** Local IDs, private fact/parameter overlay, complete effect DAG, batched identity resolution. This is the architectural step toward real multi-stage Cell completion.
4. **Connect persistent source markers and event-produced dirty inputs (S06, S10–S11, S21).** Current measured input has no source markers. Retain the old artifact as control and report both inputs explicitly; do not silently change a benchmark under the same label.
5. **Compile the device phase dependencies (S01–S05).** Reuse buffers and command sequences, reduce private-region host handoffs, fuse safe producer/consumer steps, reduce polling. Increasing quantum alone does not remove device-side barriers or global maintenance.
6. **Tune local scans, contention and counters (S07, S15–S16, S22).** Use measured segment sizes and per-key skew; do not create larger batches full of inactive work merely to advertise parallelism.

For each change record: whole inference time; outer/private waits separately; active phase count; scalar index rows; duplicate callbacks by policy; effects per wave; local outputs forwarded before publication; seed visits; actual conflict-group sizes; numerical/guard audit and per-Run repeatability. Keep final equality-partition differences explicit. No new benchmark was run for this inventory.
