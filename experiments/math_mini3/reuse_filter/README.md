# Storage template reuse filtering

Storage admission defaults to at least two distinct binding packets in the current
batch (`EGG_LAYOUT_MIN_TEMPLATE_REUSE=2`). Setting it to 1 restores unrestricted
packing. A skipped instance stays in native storage; existing virtual rows are
not unnecessarily materialized. Notification packets are still drained.

Proven closure macros are retained. An initial experiment delaying low-frequency
closure learning reduced the library but made warm continued learning slower
(15.93 to 19.81 ms in that exploratory run). That policy was discarded. The final
change filters storage only, without changing the inference schedule or library.
No historical binding set is retained to calculate frequency across rounds.

## Fixed-library ablation

Same round-2 library, mini3, three independent processes for each threshold, rotated
order. Every round's match/e-node/e-class counts agree across thresholds. Final
state counts are 211 cumulative matches, 347 e-nodes and 137 e-classes. Count
agreement is not by itself an isomorphism proof; native storage regressions cover
the representation changes. The executable library remains 22,017 bytes.

| Minimum batch instances | Driver median | Packing median | Peak RSS median | Final virtual rows |
|---|---:|---:|---:|---:|
| 1 (control) | 17.58 ms | 1.74 ms | 10.20 MiB | 311 |
| 2 (default) | 16.12 ms | 1.42 ms | 10.25 MiB | 302 |
| 8 (experiment) | 14.61 ms | 0.84 ms | 10.00 MiB | 276 |

Driver timing includes diagnostics and per-round file output, excludes final
report serialization and engine destruction. RSS is process peak from macOS
`time -l`. These are small-sample observations, not confidence intervals or a
universal choice of threshold 8. Default 2 expresses the minimum repetition
requirement, not a predicted memory break-even point.

Per-template reports now include distinct batch instances, admission decision,
distinct native rows covered, and net newly virtual rows. The latter is affected
by overlaps and existing storage ownership. For example, control round 3's rule
0 sees 31 instances but adds zero virtual rows. High frequency alone therefore
does not establish storage benefit. These counters are not net RSS attribution.

One full 11-round run at threshold 2 still hit the external 4-GiB guard during
round 11: observed process-group RSS 4,344,971,264 bytes at 9.161 seconds. This
optimization does not solve the full workload's memory explosion.

```sh
cargo build --release --offline --example adaptive_math --bin egg_layout
python3 tools/bench/reuse_filter.py --output out/reuse-filter-new \
  --library experiments/math_mini3/adaptive_runtime/bake_lib_round2.json --full

# Optional stronger filter; inference macros are unchanged.
EGG_LAYOUT_MIN_TEMPLATE_REUSE=8 cargo run --release --offline -- \
  reuse-adaptive frozen experiments/math_mini3/source.egg out/filter-eight \
  experiments/math_mini3/adaptive_runtime/bake_lib_round2.json
```

Eight adaptive runtime tests pass, including singleton rejection, repeated-instance
admission and preservation of proven closures. Earlier benchmark results remain
unchanged; `measurements.json` contains this ablation's raw runs.
