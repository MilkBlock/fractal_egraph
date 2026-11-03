# Tool semantics

Validated with Xcode/Instruments 27.1 on M3 Max (40 GPU cores). Read local tool
help and `profile ?` if the version changes.

## Three views

`xctrace` GPU + Metal Application records CPU encoding, submission and GPU
execution. Labels identify Run, ruleset, kernel and launch width. Depth-0 GPU
intervals are used for queue accounting; nested intervals are not added again.
Split encoder segments are summed, not replaced by their envelope including
preemption gaps. Driver fusion may combine multiple named kernels in one event.

Metal GPU Counters + Shader Timeline records occupancy/resource samples.
`gpu-counter-info` supplies Apple's units and definitions. The large derived
counter XML is streamed; averages use duration overlap with the target's active
GPU intervals or sampled shader timeline. Preserve each metric's covered time.
Other processes can contribute to these device-wide counters. A shader timeline
may miss a long tail on unsampled cores; never treat its relative share as exact
wall-time attribution.

MTLCaptureManager + `gpudebug` profiles isolated replay. The copied worker starts
capture for exactly the selected Run, only in capture mode. This avoids relying
on present/draw boundaries in a compute-only program. `gpucapture` can also attach
to a capturable process after discovering its capture boundaries. Capture mode
requires `METAL_CAPTURE_ENABLED=1` (set by the script).

Live execution time, replay Duration, sampled Cost and static shader properties
are different quantities. Default replay GPU state need not equal live clocks.
Keep the tool's sampled-core count. Do not infer an end-to-end speedup from replay.

## Manual gpudebug navigation

Prefer `capture_report.py`: it owns and cleans its session, ranks by duration,
collects counter groups and shader properties, and embeds/reuses profiles.

```text
profile ?
profile list
profile load                        # reuse an embedded profile
profile run --gpu-state default --exec overlapping
go performance
info --all
go encoders
list --all
go ceN                             # discover N from list
info --all
go counters
go performance_limiters
list --all
go ..
go memory
list --all
...
profile embed                      # new profile only
exit --terminate
```

From `performance`, `go shaders` then `info compN` reports registers, spill and
instruction mix. Relative paths and `go ..` work; do not assume leading slash
resets the tree context. `:d` follows a listed cross-tree link to the command.
The prompt is not JSON even with `--json`; helpers extract objects separately.

## Choosing a follow-up

- Long single-thread kernels: identify independent effects before parallelizing;
  preserve merge, subsume and Run visibility requirements.
- Large grid but low occupancy: inspect binding-work imbalance, registers and
  spills. Dispatch size does not measure sustained active work.
- High occupancy, low ALU, high memory limiter: reduce bytes and irregular access;
  adding threads or unrelated GEMM does not remove the bottleneck.
- Many tiny commands and gaps: inspect CPU wait samples and submission boundaries
  before batching or indirect dispatch.
- No FP16/FP32 instructions in the hotspot: this path does not exercise matrix
  arithmetic hardware. A new representation must remove useful work rather than
  merely inflate utilization.

Use the existing benchmark skill for any optimization's speedup and full graph /
extraction checks. This skill diagnoses costs; it does not certify equivalence.
