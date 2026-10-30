# Resident measurement protocol

Use this for `generic_cell.transport_version == 7`. Work on the real executed
artifact, not the current compiler's reconstruction of a historical program.

## Full encoder profile

`resident_full.py --run ATTEMPT --build BUILD --output NEW` requires the saved
`program.json`, uninstrumented `result.json`, and matching base worker plus its
materialized Swift source in BUILD. It verifies their hashes before compiling a
diagnostic worker. The input JSON, shader, control, quantum, private schedule
eligibility and storage layout are unchanged. Stage sampling separates encoders
inside the original command buffers. It covers ordinary AND private-schedule
encoders, their otherwise unreported waits, watermark copies and history reads.

The instrumentation allocates timestamp buffers in pages (16 KiB each), because
the inspected M3 Max rejects a single buffer above32 KiB. More encoders allocate
more pages; this does not cap or sample away work. Resolved timestamp values are
nanoseconds, not Metal4 raw clock ticks. Invalid/missing timestamps or an interval
union larger than command durations fail accounting rather than becoming zeros.

`result.json.full_encoder_profile.commands` contains every registered command,
its GPU interval, CPU encode/wait wall duration, per-phase encoder times and its
`outer_before` coordinate. `run_before` and `run_after_observed` are controller
observations, not exact attribution of every kernel to a logical Run. Private
programs can complete multiple logical stages without advancing the outer count.
The summary keeps phase totals and p50/p95/p99/max, issued and zero-duration
encoder counts, private execution counts and complete-state verification.

Verification compares all instances (rows, IDs, values, visibility, parent
forests, history, watermarks and work counters), program identity, private
execution and outer dispatch count against the saved control. This is exact
same-executor replay validation, not an independent proof of egglog semantics.
The diagnostic worker hash is distinct from the base worker hash. No checkpoint
is mislabeled as originating from the base executable.

A full trace can perturb time. Always display the measured perturbation ratio.
Do not turn these phase shares into an exact partition of the uninstrumented
runtime. Command time not covered by encoder intervals includes unmeasured blits,
encoder transitions, sampling costs, resource/scheduling waits and other gaps;
it is not automatically a barrier bill. CPU wait wall time overlaps GPU work.

## Live hardware windows

Read `outer_before` from the full profile and select explicit bounded windows:

```sh
python3 scripts/resident_window.py --run ATTEMPT --build BUILD --output NEW \
  --mode counters --window 1352:5 --window 1888:12
```

These coordinates are an example from one saved build, not universal raytrace
Run numbers. Windows must be sorted, non-overlapping and within that control's
outer command count. Multiple windows share one complete execution. The worker
pauses before/after each window only while the controller starts/stops xctrace;
Darwin readiness notification determines release, not an arbitrary sleep.
Original command/encoder grouping is retained for live counters. A grouped
encoder is called `resident_pipeline`, not falsely attributed to its first
kernel. Sampled shader intervals provide finer correlation where available.

The default `Cell Metal Limiters` template must actually exist and include Time
Profiler. Waiting-thread recording is enabled through the complete exported
recording-options JSON. `.trace`, exports and TMPDIR can be placed on the user's
external disk. Apple-owned caches may still use internal storage.

Small provenance/replay/window summaries are mirrored to the repository's
`out/profile-summaries/` after each completed stage. This preserves useful data
when a removable volume disappears. `artifacts.py mirror --run SAVED_ROOT`
also recovers summaries without rerunning inference. `resident_report.py`
checks the unchanged saved reference and full result, reuses complete summaries,
and streams missing counter XML directly from xctrace stdout. It does not need
to retain a multi-GB counter export. Keep the original `.trace` for reinspection.

Each window records exact controller coordinates, PID and tool command. The
whole program then finishes and is compared with the saved control. A missing
window, failed trace, unexpected issue, missing samples or endpoint difference
is not accepted. The result wall time includes deliberate pauses and is never a
benchmark. Hardware counters remain device-wide; overlap with a target shader
is not exclusion of WindowServer or other processes. Preserve per-counter
covered seconds and sample counts. A short window does not estimate full-run
average occupancy or bandwidth.

## Bounded GPU capture

Use the same window runner with `--mode capture`; it preserves full-schedule
execution but splits phase encoders inside the chosen window for attribution.
Run `capture_report.py --trace NEW/window-0/window.gputrace --output ANALYSIS`.
It saves Apple replay Duration, sampled counter limiters, registers, spill and
static instruction mix when available. The capture window can start mid-Run at
a completed command buffer. Report the recorded coordinates rather than calling
it one complete Run. Reuse embedded profile data with `--reuse`.

Do not reinterpret Apple's Cost% as elapsed-time share, static instruction count
as dynamic work, or grid width as sustained occupancy. Apple may sample only a
subset of shader cores; keep that coverage. Diagnostic replay clocks and live
execution clocks are distinct. Failure of capture/replay does not invalidate a
separately verified live timestamp/counter result, but the unavailable fields
remain missing.

Stage capture on APFS. On 2026-10-30, the capture written directly to exFAT
crashed Apple's decoder (`DecodeDYMTLDeviceDescriptor`); copying those same bytes
to APFS did not repair it. Recapturing the same verified window directly on APFS
was readable. This does not establish a hardware fault or universal filesystem
incompatibility. Archive only after capture/profile has finished; use
`artifacts.py archive` to hash-check every copied file and retain the source.

Apple's profile may successfully embed/load, yet expose no per-encoder counters
or shaders. Timeline records can then report `Unknown`, zero kernel time, or a
whole-frame placeholder duration for most encoders. Do not rank these as hot
kernels. `capture_report.py` retains partial aggregate data and fails explicit
attribution. A serial replay is a distinct diagnostic, not a faster benchmark.
The command tree still records actual dispatch shape even in this situation.

## Minimum useful output

- Clean historical baseline with verified input/executable identity.
- Diagnostic runtime and perturbation, full/private path checks.
- Phase times and call/duration distributions; classify all phases or retain an
  explicit unclassified group.
- Command, encoder and CPU-wait totals with overlap definitions.
- Work counts from the same control: matches/bindings vs actual requests/rows,
  Cell waves, ordinary RHS bindings, index refreshes and scanned rows.
- Window-specific occupancy, ALU, bandwidth and limiter samples with units and
  sample coverage; CPU wait stacks, not just a scheduler Running label.
- Captured hot shader dispatch shape, register/spill/instruction facts when
  available; never fill unavailable fields with zero.
- Input/worker/profiler/tool hashes, window coordinates, failed attempts and the
  complete-state comparison result.
