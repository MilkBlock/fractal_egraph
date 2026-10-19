# Full Pass-3 joint sat-skip: no benefit in the current implementation

This measures the **entire captured raytrace Pass-3 main schedule**, following
its setup/initialization. All four lanes use the same structural stable-choice
policy and current native fork. This is not pristine upstream, the isolated
terms sub-schedule, or a new three-pass frontend build.

One release run per lane, sequentially, one native thread, query audit disabled.
Build, shared setup, worker setup, verification and serialization are outside
the inference timers. Stable-key maintenance during the schedule is included.
The control GPU worker is released before the joint GPU lane starts. The joint
observer and its actual mirror/guard maintenance execute inside the timed run.

## Timing

| Lane | Complete main schedule (s) |
|---|---:|
| Fused native | 24.103171 |
| Same-submit CPU control, no joint | 24.932210 |
| GPU, no joint | 19.025306 |
| GPU + joint sat-skip | 28.111245 |

The GPU matcher without joint is **1.3105x** faster than the same-submit CPU.
Enabling joint gives **0.6768x** relative to that GPU control: **47.76% slower**,
an extra 9.086 seconds. GPU + joint is **0.8869x** relative to same-submit CPU,
or 12.75% slower. These are single-run observations, not averages.

## Admission explains the result

The observer encountered 12 joint terms saturation boundaries:

| Counter | Result |
|---|---:|
| Materialized closure macro instances | **0** |
| Skipped joint saturations | **0** |
| Quiescent skips | 0 |
| Fallback boundaries | **12** |
| Reason: removal or subsumption history requires ordinary closure | **12** |

By the time the full schedule reaches its terms boundaries, the observer has
seen row removal/rebuild/subsumption events. The current `saw_removals` contract
conservatively disables materializing macros for the whole observer thereafter.
It does not distinguish an irrelevant canonical-key replacement from a genuinely
invisible candidate that would invalidate a local transfer. This guard is needed
by the current proof; deleting it without a replacement proof would be unsound.

The observer nevertheless keeps receiving native events, maintaining its mirror,
checking the environment at saturation callbacks and recording completion.
The measured pre-boundary hook time is 3.747 s; shared event collection is
0.974 s with joint versus 0.252 s without. These are partial stage timers and do
not account for all of the 9.086 s difference. In particular completion callbacks
and effects on the rest of execution are not separately attributed here.

Thus the earlier initialization-only result—4,077 macro instances and 16,351
skipped smooth bindings—does **not** transfer to this full Pass-3 run. The current
joint implementation has zero useful admission on this workload and adds cost.

## Correctness and actual extraction

- Same-submit CPU, GPU without joint and GPU with joint have identical complete
  original facts, primitive values, equality partitions and visibility:
  **699,069 rows / 524,631 mapped e-classes**. No table is excluded.
- All assisted paths execute 10,550 scheduled ruleset runs. No extra semantic
  rounds are used to improve the result.
- Actual production Tiger runs on the saved native/CPU/GPU-with-joint graphs.
  All three extracted outputs are byte-identical and reconstruct the same
  **29 functions / 8,018 public selected rows**.
- Output SHA256: `c48fe2b212d341b1c3f3de3ef94e77e3252b661e68be3bf60fbd417bb5ad3ce9`.
- Tiger time: native 3.022 s, CPU 2.817 s, GPU-with-joint 3.010 s. Including Tiger,
  CPU takes 27.750 s and GPU-with-joint 31.122 s. Serialization is still excluded.

As in the previous stable-choice report, the fused native lane retains different
intermediate Smaller/TCPair facts from the sorted-submit lanes; their strict
whole-graph comparison fails although extracted IR is identical. Consequently
the verified speed ratios above use the same-submit CPU and GPU controls, not
fused-native graph equivalence.

## Reproduce

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py \
  run --joint-sat --output out/raytrace-pass3-joint-new
```

The skill runs all four inference lanes, enforces both graph gates, then performs
Tiger extraction. Saved measurements, hashes and commands are in
[results/joint-sat/report.json](results/joint-sat/report.json) and
[results/joint-sat/workflow.json](results/joint-sat/workflow.json); quality and
extraction timing are under that directory's `extraction/` subdirectory. Large
graph exports stay in ignored `out/` and are not committed.

No admission guard or optimization was changed to improve this measurement.
The next necessary change is a local visibility/version proof that can survive
unrelated rebuild history, together with avoiding unnecessary observer work for
unsupported boundaries. Neither change is part of this benchmark result.
