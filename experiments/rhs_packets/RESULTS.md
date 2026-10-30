# RHS publication results — 2026-10-30

Implementation: `42883fc`. Actual resident GPU backend, source markers disabled, existing multi-stage Cell program enabled. Historical controls are reused; these small Pass 3 timings are not same-binary paired measurements. This initial validation did not rerun Raytrace; the subsequent full measurement is in [RAYTRACE.md](RAYTRACE.md).

| Complete Pass 3 | Historical s | New s | Reduction | Final rows | Runs | Packet dispatch quanta |
|---|---:|---:|---:|---:|---:|---:|
| fact | 0.324846 | 0.306930 | 5.52% | 1089 | 407 | 0 |
| loopfact | 0.358976 | 0.347834 | 3.10% | 1038 | 388 | 0 |
| sum-bits | 1.053600 | 0.944570 | 10.35% | 3258 | 835 | 0 |

All three retain **identical raw facts, values, parents, fact identities, visibility, watermarks, complete Run history, and private-schedule work** relative to the saved old GPU outputs. Each new encoder profile also independently reproduces its clean run. These are GPU comparisons, not a new full native-CPU Pass 3 validation.

All three have **zero new RHS packet work**. Their improvement belongs to omitting the idle writer dispatches, **not** Cell closure reuse or constructor batching. The existing 256-binding batching grain is retained. The small 257-Cell, three-step cone test takes 0.03241 s off / 0.03269 s on; it establishes correctness, not a speed claim.

## Large nested-constructor workload

One same-worker off/on pair, 32,768 complete bindings: **0.446127 → 0.076150 s (5.86×)**. Full raw GPU state and Run history are equal. This uses the actual resident engine but is synthetic, not eggcc or Raytrace. Compilation/setup is excluded. The measured pipeline includes matching, publication and completion, not just a standalone kernel.

## Validation and limits

- 37 relevant compiler/IR/reproducibility tests; 15 profile tests.
- 12 actual runtime cases: native facts/equivalence/visibility checks, stable GPU replay, finite Run visibility, late union, custom merge ordering, constructor prediction, hidden intermediates, independent arenas during growth, and overflow retaining its preceding effects.
- Pure-constructor multi-stage cones are compiled from existing analyzer scope/wiring evidence. Function-update/union cones still use the existing continuation path; noncommuting residual effects remain ordered.
- An early 16-kernel draft regressed fact to 0.415 s. After fusion, an always-dispatched draft still took 0.479 s. Both were rejected; the final writer has 11 kernels and is issued only after GPU readiness at an existing host quantum boundary.
- A machine restart interrupted an earlier sum-bits shader compilation. The interrupted run is excluded. The final three measurements above completed after restart. First-time Metal setup remains slow; its time is not inference time.
- No new hardware occupancy/bandwidth measurement was made. Encoder profiling perturbs these small workloads by approximately 4× and is not used as a clean-time decomposition.

Reproduce and compare without writing a new analysis script:

```sh
cargo profile
python3 experiments/rhs_packets/report.py
```

The full exact counters/provenance are in [results.json](results.json), with the compact Pass 3 comparison in [pass3.json](pass3.json).

The cached-only verification hit all 12 stages and ran no compiler/GPU workload (1.07 s driver time).
