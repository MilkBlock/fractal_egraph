# Dynamic context performance, 2026-10-26

**GPU replay is fast, but dynamic preparation currently removes the net gain.**
This measures the standalone `is-resolved` executor on real Raytrace Pass-3
changes. It does not measure a complete GPU Pass 3, a production event adapter,
incremental publication into egglog tables, or pristine upstream egglog.

## Contract and result

One run per configuration, sequential, on the same M3 Max. No Metal validation,
per-Run graph export or GPU-state readback inside timing regions. The immutable
input is captured separately with native verification. All four timed GPU lanes
preserve Run boundaries and end with byte-identical truth, aliases, visibility
and birth timestamps. The native timing control passes the full typed graph
check: **697,922 rows / 524,657 mapped classes**.

The native control replays all 10,121 verified Runs in one process without
intervening exports/IPC. Below, native time includes only the same **782
`is-resolved` Runs**. External phases and extraction are not charged to that row.
The GPU rows include applying all 16 actual context updates, but exclude input
acquisition, CPU wiring compilation and worker initialization.

| Execution of the same selected work | Host elapsed, including device updates | GPU command time within it |
|---|---:|---:|
| Native egglog, current kernel | 506.3 ms | — |
| Matrix, one call per Run | 484.7 ms | 65.5 ms |
| Direct GPU conjunction, one call per Run | 422.4 ms | 32.8 ms |
| Matrix, coalesced consecutive Runs | **140.5 ms** | 60.6 ms |
| Direct GPU conjunction, coalesced | **102.8 ms** | 29.9 ms |

Coalescing changes host command granularity, not rule visibility. Double-buffered
state still advances one Run at a time. Groups never cross a patch, an intervening
native ruleset, or a quiet boundary. In this saved schedule, 782 calls become 36;
the source-accounted execution command buffers decrease **1,580 -> 177**. The
eight-Run device batch size is unchanged. This is a legal replay experiment,
not automatic production scheduling or advance knowledge of future facts.

Additional costs measured separately:

| Preparation | Time |
|---|---:|
| Typed template admission | 0.4 ms |
| Initial context compilation | 329.0 ms |
| Later incremental context compilation | 306.9 ms |
| Matrix worker startup/pipeline setup, batched lane | 42.9 ms |

Thus **admission + all wiring + batched matrix replay = 776.8 ms**, versus the
native selected 506.3 ms. This is already **1.53x slower**, even before worker
startup, event delivery and native table handoff. Initial compilation might be
amortizable in another deployment, but is real work in this first execution.
The 3.60x native/replay phase ratio must not be called an end-to-end speedup.

## What is actually wrong

1. **CPU wiring work is larger than GPU execution.** The Python assembler takes
   635.9 ms to process 57,584 affected anchors. Two later changes account for
   233.2 ms, **76.0% of later compilation**: 9,699 anchors after structural growth,
   then 11,295 after 3,006 fact-coordinate merges. Typed dictionaries, deep row
   copies, alias subscriptions and clause/support-set reconstruction are real
   costs. We have not profiled their individual shares; this measurement locates
   the expensive batches, not a single Python instruction.

2. **Small host requests dominate the current step path.** Matrix Run calls take
   448.5 ms, but their GPU command intervals total 61.6 ms. The 386.9 ms residual
   includes encoding, scheduling, waits and pipe/JSON status handling; it is not
   all data-transfer time. Coalescing reduces that residual to 50.5 ms, while
   GPU Run time changes only from 61.6 to 57.5 ms. Dynamic updates themselves cost
   32.5 ms host / 3.1 ms GPU in the coalesced lane.

3. **The current matrix layout has too little shared arithmetic.** It is an exact
   conjunction expressed through a certified ridge separator, with 12.5% nonzero
   block-diagonal weights. The direct GPU control uses the same grounded contexts,
   alias/visibility state and publication semantics, yet its GPU command time is
   roughly half. Coalesced host replay is 26.8% shorter as well. This demonstrates
   a problem with this particular representation, not that matrix execution is
   intrinsically unsuitable for richer shared rule computations.

4. **Retiring invalid contexts does not avoid repeating valid ones.** The layout
   executes 23,130,076 context-record visits across the 782 Runs, and performs
   23,230 new state-bit publications. These are source-derived scan counts and
   GPU state updates, not native candidate-row counts, rule matches or committed
   e-node insertions. Current banks scan all valid records each round. Dormant
   contexts cannot just be deleted: they may become enabled later. Dependency
   frontiers or activation of affected matrix blocks would address this work.
   Nominal padded GEMM work is 869,207,040 FLOPs, including zeros; it is not a
   useful-throughput or hardware-occupancy measurement.

5. **Scope is too small for a large overall improvement.** The current native
   replay takes 23.135 s before extraction, and `is-resolved` accounts for only
   **2.19%**. Even making this phase free saves at most that fraction of this
   replay. The larger measured rulesets are `always-run` (6.158 s),
   `terms-helpers-helpers` (5.767 s), `terms` (2.898 s) and `terms-helpers`
   (2.321 s). They are not covered by this unary-existence closure executor.

## Storage, transport, and integration gap

Coalesced execution sends **980,140 bytes** and receives **20,143 bytes** of status
over the whole replay, excluding the final audit. Final context buffers occupy
1,782,272 bytes; the worker reports 2,883,584 bytes of Metal allocation. These are
not process RSS. The test performs eight buffer resize operations; it does not
copy the whole egraph per Run.

The validation-only native snapshot adapter still costs **87.956 s**, scanning
11,751,307 rows to capture the events. That is excluded from replay and explicitly
remains a deployment blocker. It must be replaced by committed row/alias events.
Similarly, reconstructing the final graph for verification is not a timed,
integrated incremental writeback into production tables. No whole-engine speedup
is claimed, and no Cell proof credit is inferred from general fixed-wiring reuse.

Next priorities: consume real commit events; reduce affected-anchor compilation;
integrate legal multi-Run execution; activate only affected context blocks; then
design denser shared arithmetic for the larger rulesets. Merely widening the
current block-diagonal matrices adds zero work rather than reuse.

## Reuse the benchmark

The bench follows the `cell-assisted-egglog-bench` protocol for single measurements,
separate diagnostics, preserved controls and complete result checks. This new
standalone executor has its own adapter; do not route it through the unrelated
full-GPU matcher or count its timings as Pass-3 acceleration.

```sh
# One-time validated event capture; no complete graph snapshots are persisted.
MTL_DEBUG_LAYER=1 python-with-numpy experiments/matrix_closure/verify_dynamic_raytrace.py \
  --source PASS3.egg --frontend FRONTEND --worker DYNAMIC_WORKER \
  --output out/dynamic-timing-input --capture-workload

# Validation off for timing. All four configurations run once; no averages.
python-with-numpy experiments/matrix_closure/bench_dynamic.py \
  --reference out/dynamic-timing-input --frontend FRONTEND --worker DYNAMIC_WORKER \
  --output out/dynamic-timing
```

For later compatible comparisons, `--reuse-native out/dynamic-timing` reuses the
passed CPU control only after binary, source and exact native schedule checks.
An unchanged result should simply be read, not rerun. Runtime source and binary
changes require a matching verified capture. Compact numbers and provenance are
in [`performance-results.json`](performance-results.json); raw reports and event
packets stay untracked under `out/matrix-dynamic-perf*`.
