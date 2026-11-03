# Why the resident engine is slower than both previous backends

2026-10-30. Source and retained-result inspection only; no new performance run.
This supplements `SYNCHRONIZATION_AUDIT.md`. Do not treat the three execution
architectures below as one backend with a Cell on/off flag.

## Three distinct implementations

| Implementation | Retained full Pass-3 inference | Execution ownership | Existing acceleration |
|---|---:|---|---|
| Assisted runtime | 12.340255 s | GPU residual matching; native CPU action/merge/rebuild; CPU schedule and Cell services | Shared loop-prefix markers, parameterized queries, joint closure services |
| Device backend | 42.336022 s in `out/cell-spines-pass3-full/report.json` | GPU matching/actions/merge/rebuild; host dispatches Runs and rebuild continuations | Specialized joint terms program, generic matrix/comparison closure, stage patches |
| Resident ABI7 | 185.707129 s in the previously audited `cell-waves-pass3-final` run | GPU control tape, global arena, generic Cell contexts, per-effect wave publication and residual matching | Generic recipe successors, one private fixed-carrier schedule program; no source marker in this measured input |

The older device `out/multi-stage-pass3-complete/report.json` is 39.797905 s.
It is a separate historical run, not a newly chosen faster matched control.
The 42.336022 s report is useful because it records the later Cell-spine path.
Do not compute causal optimization credit from differences across these versions.
They differ in lowering, services, control execution and final graphs.

The 12.34 s evidence is the retained report at
`/Users/mineralsteins/.codex/worktrees/automatic-refutation/egg_layout/experiments/loop_marker_regression/README.md`.
The bench skill history command currently finds no reusable raw full-run cache
in that checkout; it must not manufacture one or silently remeasure it.
That report records 699,068 rows / 524,631 mapped classes and same-submit CPU/GPU
and Tiger equality, with a separately disclosed fused-native mismatch.
The device multi-stage report records 699,784 rows; current resident has 701,998.
These are not identical endpoints. Resident final partition differs even from
its own disabled historical control; numerical/guard checks and GPU per-Run
repeatability passed separately.

The external SSD's previously inspected resident directory became unavailable
during this comparison. Current resident configuration/counters are those
recorded in the preceding audit; the old device reports and generated shader,
old assisted source, and current resident source were inspected directly here.
A fresh cross-artifact core comparison was therefore not completed.

## What the 12.34 s timer actually included

From the retained regression report:

- Complete inference: 12.340255 s.
- Native RHS including rebuild: 3.388806 s.
- Worker round trip: 3.868540 s.
- Commit/event handling: 1.365782 s.
- Cell query handling: 0.653074 s.
- Saturation hook: 1.769594 s.

Some timers are nested; do not sum them into a reconstructed total.
`cell-assisted-egglog/src/table_live.rs::run_set` collects Cell bindings, asks the
GPU for remaining bindings, then invokes `apply_completed_cell_ruleset` on the
native engine. The commit feed maintains the shared prefix state. These were
runtime markers: the original `.egg` remained unchanged and inspection-only
marker definitions were not scheduled as extra scans.

Thus the native implementation's CPU serial portions are not equivalent in cost
to moving the same loops to one GPU thread. GPU ownership by itself supplies no
performance guarantee, and disabling Cell in the new executor does not restore
the old native storage/action implementation.

## A concrete regression beyond missing Cell coverage

The device backend already supports pausing a custom merge when its constructor
index becomes invalid:

```
save duplicate-merge cursor
  -> parallel canonical-key hash index refresh
  -> resume the same callback order
```

Source: `engine/metal/rebuild_parallel.metal:91`,
`engine/metal/worker.swift:167-169`.
The actual saved `out/cell-spines-pass3-full/kernel.metal` retains that branch.

The resident adapter explicitly removes it:

```
# eggfuse/compiler/targets/metal/resident_rebuild.py:39
body = re.sub(... PARALLEL_MERGE_INDEX ... return ..., '', body)
```

Callbacks can then reach `row_lookup -> ensure_row_index`, whose invalid-index
case loops over every live row on the calling thread. This is a lost existing
optimization, not a hypothetical future improvement. The previously audited
resident generated shader confirms the branch is absent.

Historical one-Run evidence: `experiments/device_backend/merge-index-results.json`
measured Run 6,232 from the same saved device input: GPU time 7.246608 ->
0.141713 s with the resumed parallel refresh; typed rows/values/equivalence/
visibility agreed. That does not quantify its share of the current 185.71 s,
but it makes restoring the mechanism a clear migration priority.

## Serial / synchronization comparison

| Operation | Assisted 12.34 s | Older device ~40-42 s | Current resident 185.71 s |
|---|---|---|---|
| Context discovery | Shared CPU prefixes and event-driven query services; remaining GPU joins | Prepared query indexes and typed seed routing; special local programs | Source markers absent in this timed artifact; independent binding/context preparation remains |
| Query routing | Persistent event/table family shapes | Separate compiled ruleset kernel, switch on seed table | One resident match entry, switch on ruleset then generated per-rule seed guards; does not reuse the old typed seed switch |
| RHS granularity | Full RHS via optimized native engine | Full compiled RHS/ordered commit; specialized closure completion | Eligible contexts stop at each real pending global effect; repeated key grouping/allocation/index/reply waves |
| Mutable merge | Native data structures, no Metal launch per callback operation | Ordered callbacks with explicit parallel index-refresh continuation | Ordered callbacks with that continuation removed; implicit scalar full index refresh remains reachable |
| Global maintenance | Native rebuild; committed deltas update GPU mirror | GPU rebuild and query patches, host submits necessary branch | GPU rebuild plus Cell continuation/packet/receipt machinery and broad fixed phase schedule |
| Synchronization | CPU/GPU query round trips and native commit boundaries | Per-Run dispatch/waits; 3,806 rebuild waits in the 42.34 s report | 2,971 outer waits plus private schedule waits; 9,971 effect waves and device barriers |
| Multi-stage reuse | Joint closure observer plus query reuse | 12 joint terms invocations / 2,826 stage patches, plus matrix closure | 36 fixed-carrier private regions / 782 private Runs, generic successor templates; legacy joint terms/comparison paths disabled |

Counts in different columns have different meanings. In particular 2,761 old
assisted GPU dispatches, 5,697 old device commands, and 2,971 resident outer
command buffers are not the same unit. Private Runs cannot be equated with
stage patches or distinct live Cells.

The 42.336022 s device report breaks GPU time into prepare 4.356377 s, matching
9.543871 s, RHS 12.924087 s, rebuild 3.580944 s, for total 30.405280 s. Its many
host waits were expensive, but its GPU work was much shorter than the resident
reported 182.760060 s. The resident problem therefore cannot be reduced to
CPU round trips. New per-phase timings are still required for exact attribution.

## Why marker-to-Cell was not the timed path

The initial unified Pass-3 compiler admitted no offered source view from its 72
observed Cells. Subsequent `0f1fd44` repairs generated 48 prefix/structural tables,
112 consumers and 707 producer publication actions; eight runtime fixtures and
compiler checks passed. See `experiments/spine_table_state/README.md`.

The publication experiment retained the old unmarked typed input for an isolated
publication comparison. That is legitimate for that narrow experiment, but it
is **not completion of the requested marked full pipeline**. The existing ~40 s
GPU joint-terms/comparison optimizations were also disabled during migration to
generic recipes without providing equivalently efficient general lowering.

The goal of removing dedicated terms handling remains reasonable. The migration
must express its useful transformations as typed compiler passes, not remove
its compiled work reduction while retaining every generic maintenance cost.

## Corrected implementation order / acceptance boundary

1. Restore existing general backend capabilities first: resumable parallel merge
   index refresh, typed seed routing, efficient ordinary RHS fallback. A generic
   wrapper must not silently turn these into scalar whole-graph work.
2. Use the actual marked compiler artifact for full-pipeline experiments. Report
   marker tables, consumer hits and stage forwarding. An empty marker contract
   must be explicit; it cannot be reported as the requested optimized pipeline.
3. Compile the old joint-closure techniques into general rule-derived passes:
   shared parameter/dataflow, exact comparison reduction, local result forwarding,
   complete effect recipes and deferred global identity resolution. No rule-name
   hardcoding and no forced enablement of an unsupported special program.
4. Inside the SAME executor compare ordinary execution with Cell execution,
   keeping typed source, marker variant, query routing, index maintenance and
   precision identical. Source-marker on/off is a separate experiment, including
   publisher maintenance cost. Report soundness checks and per-Run repeatability;
   do not hide graph/quality differences.
5. Retain the ~12 s assisted and ~40 s device records as explicit historical
   references. Do not treat 198 -> 185 s inside a slower new implementation as
   progress against the original speed target.

This comparison changes no runtime code and reports no new measured speedup.
