# Effect-prefix rejection: correct subset, no net Pass-3 gain

This branch preserves a rejected performance experiment. It is not the default
Cell runtime. Reproduce its complete five-lane comparison with the repository's
`cell-assisted-egglog-bench` skill, `run --joint-sat`.

The compiler derives a read-only RHS program for constructor lookups and unions.
It rejects primitive bodies/actions, set, delete/subsume, panic and unknown
function effects. Existing rows and already-equal union targets can certify a
no-effect substitution. Source rule cursors remain authoritative. A depth-marked
prefix cursor prevents stale bindings after backtracking; decomposed queries
retain the late guard. The triangle regression measures fewer actual Join
probes and at least one rejection before a remaining Join stage.

Early deletion of no-op lanes changes vectorized action batching. Even with
identical semantic values, unrestricted prefix pruning produced one extra
reversed Smaller record. Numeric closure admission also must distinguish an
unfiltered full-LHS match from an effectful match. A private engine-local
observation counter and a short-circuit compiled existence query preserve that
distinction. The final variant additionally permits early rejection only when
at most one RHS instruction can mutate the graph; RHS lookups already witnessed
in the LHS are discounted. Other admitted rules use late rejection.

Complete saved raytrace Pass 3, single release execution per lane:

| Lane | seconds |
|---|---:|
| Fused native | 24.987 |
| Same-submit CPU | 25.226 |
| Original GPU, no sat-skip | 19.028 |
| GPU plus effect guards, no sat-skip | 20.796 |
| Joint Cell path plus effect guards | 16.829 |

All required strict public row/value/equivalence/visibility gates pass in
`out/raytrace-effect-single-write-v1/report.json`. This run did not do a new
Tiger extraction. The effect-only control regresses; the joint/raw-GPU ratio is
only 1.131x. Do not credit the 1.236x ratio against the slower effect-only control
as an overall improvement. Earlier failed reports are retained at
`out/raytrace-effect-terminal-v1` and `out/raytrace-effect-witness-v1`.

The production baseline before this experiment (`1a83ccc`) measured 15.159 s
versus 18.332 s GPU-off with strict graphs. These are separate one-shot runs,
not a statistical comparison; the experiment provides no evidence to justify
its added default complexity. The default branch will retain the earlier path.

This is actual egglog execution and native effects, not a standalone simulator.
Timings exclude compilation, setup, exports and verification. The fusion-native
helper footprint is already different from the same-submit protocol; it is not
the exact-graph speedup denominator. Probe counters are disabled during timing.
