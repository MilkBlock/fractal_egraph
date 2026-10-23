# Loop-marker regression investigation

Actual shared CPU/GPU runtime on the complete saved raytrace Pass-3 input,
not a standalone marker microbenchmark. Each timing lane runs once; sampling
is separate and excluded. CPU controls are reused from `pass3-compact-layout`.

## What the old numbers compared

| Saved run | Plan / implementation | Inference |
|---|---|---:|
| `out/boundary-loop` | Hand-selected loop query, original runtime | 12.455758 s |
| `out/shared-loop-private` | Two solver-selected queries, private prefixes | 14.390996 s |
| `out/shared-loop-reuse` | Same two queries, shared prefixes | 14.319296 s |

The private/shared pair tests **incremental prefix sharing**, with loop-query
takeover already enabled on both sides. It does not reproduce the old off/on
takeover comparison, nor does its 0.50% difference establish a stable speedup.
Nevertheless the increased absolute time needed investigation.

## Isolated source controls

The old manual plan was replayed with the same input, worker, submission policy
and complete-graph oracle, without changing the active working tree:

| Source | Saved directory | Inference |
|---|---|---:|
| `86c1a98` old boundary implementation | `loop-regression-old-source` | 13.599238 s |
| `79e0061` selected payload support | `loop-regression-layout-source` | 13.679409 s |
| `d543fe0` generic certificates | `loop-regression-certificate-source` | 13.278891 s |
| `3cd889c` shared-prefix implementation | `loop-regression-shared-source` | 14.223786 s |
| `3cd889c` active checkout, before isolation | `loop-regression-manual` | 14.663597 s |

All five have exactly 6,890 ruleset runs, 2,425,040 row events, 239,476 union
events, 2,761 GPU dispatches, 102,423,460 uploaded bytes, 17,424,828 downloaded
bytes and 3,302,525 parameterized keyed reads. Thus the manual query did not fall
back to GPU search and work was not lost. The solver and certificate discovery
run during setup, not repeatedly during inference.

**The original 12.46 s was not reproduced by rebuilding its source.** These
single measurements do not isolate compiler/layout/environment effects, so we
do not attribute the entire historical 1.86 s difference to shared markers.
The CPU mirror/notification path and query maintenance were the actionable
costs. A separate seven-second macOS `sample` run identified native rebuild,
row movement/allocation and observer maintenance; it is not a timing control.

## Small runtime changes

1. `Rows::change` appends invalidation notifications directly to the commit
   batch's buffer. Previously every changed row allocated a temporary `Vec`,
   grew it for reverse/projection notifications, copied it into the batch,
   then freed it. All notifications, ordering and row generations are kept.
2. The joint closure's `extendable` flag is one-way until `complete()` renews
   it. Once false, `Joint::observes` stops checking additional rows for the
   same invalidation. Mirrors, subscriptions, native effects and forwarded GPU
   events **still process every row**. A later certified boundary resets the
   flag, so future updates are checked again.

Neither change removes plans, narrows the Bake library, relaxes guards, skips
finite Run boundaries, or disables the shared-prefix DAG.

## Result and verification

`out/loop-regression-fixed` keeps the generated two-query/shared-marker plan.
After the developer-tool change, a separate unchanged `3cd889c` build used the
same selected plan and explicit Command Line Tools environment:

| Matched selected-plan comparison | Inference |
|---|---:|
| Unfixed `3cd889c`, `out/loop-regression-unfixed-control` | 12.854072 s |
| Fixed implementation, `out/loop-regression-fixed` | 12.340255 s |

The observed reduction is **0.513817 s / 4.00%**, each lane once, with identical
plans and work counts. This is the relevant repair comparison. Rebuilding the
unmodified shared implementation also did not reproduce 14.32 s; the remaining
historical difference cannot be assigned to a particular code change from these
measurements. Do not report the entire historical difference as optimization
credit, or claim a stable 4% effect from one pair.

| Phase | Historical shared | Rebuilt unfixed | Fixed |
|---|---:|---:|---:|
| Complete inference | 14.319296 s | 12.854072 s | 12.340255 s |
| Commit/event handling | 1.863007 s | 1.486908 s | 1.365782 s |
| All Cell query handling | 0.894603 s | 0.690358 s | 0.653074 s |
| Saturation hook | 2.111561 s | 1.944331 s | 1.769594 s |
| Native RHS, including rebuild | 3.737854 s | 3.423989 s | 3.388806 s |
| Worker round trip | 4.287458 s | 3.965041 s | 3.868540 s |

These are observed cross-version timings, **not** a claim that both small
changes causally account for all 1.98 s. Child timers overlap; do not sum every
row. Setup (2.774665 s), compilation and extraction are outside inference.

- Complete CPU/GPU original rows, values, equality and visibility agree:
  **699,068 rows / 524,631 mapped e-classes**.
- All 29 production Tiger functions have identical selected DAGs between the
  same-submit CPU and GPU lanes.
- Four shared prefixes still hold 84 bindings each. Ruleset runs, row/union
  events, dispatches, keyed reads and query results are preserved.
- 92 focused regression tests pass, including missing ports, function updates,
  aliases, deletion/subsume/reinsert, independent consumer watermarks, finite
  Runs, closure renewal and portable certificate loading (GPU cases enabled).
- 34,162 Cell-supplied incremental queries agree with the native oracle in
  `out/loop-regression-fixed-audit`. All per-rule work counters, event counts,
  transfer totals and the complete graph also agree with the rebuilt unfixed
  selected-plan control.
- 64 benchmark protocol tests pass. External sampling is explicitly excluded
  from timing/history reuse via `run --external-profiler NAME`; the sampled
  diagnostic above carries this label and cannot become a speedup baseline.
- The already documented **fused-native** graph/extraction mismatch remains;
  this is not a claim of pristine/fused-native parity.

Full inputs, binary/worker fingerprints, source revisions, commands, graphs and
extraction evidence remain in each directory's `workflow.json`, `report.json`
and `extraction/`. During verification the active developer directory changed
to an unlicensed Xcode installation. Final builds used the already installed
Command Line Tools via a process-local `DEVELOPER_DIR`; no license was accepted
and no system setting was changed. The Metal worker binary was unchanged.
