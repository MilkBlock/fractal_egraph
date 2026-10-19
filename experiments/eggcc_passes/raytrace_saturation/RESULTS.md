# Raytrace saturation time

One fresh production 3-pass pipeline, extraction feeds the next pass. Outermost Saturate is atomic; nested time counted once. No GPU/Cell acceleration, no coverage audit, no raw trace. Includes native per-command preparation; search+apply includes RHS.

| Pass | Saturate s | Ordinary Run s | Saturate / scheduled execution | Execution including setup s | Extraction wall s |
|---|---:|---:|---:|---:|---:|
| 1 | 4.302 | 0.086 | 98.04% | 4.476 | 1.185 |
| 2 | 12.094 | 0.164 | 98.66% | 12.333 | 3.251 |
| 3 | 22.628 | 0.494 | 97.86% | 23.225 | 3.452 |
| Total | 39.024 | 0.744 | 98.13% | 40.034 | 7.888 |

Complete CLI wall: 50.793 s. Saturate accounts for 76.83% of CLI wall and 97.48% of pass execution including setup.
Serialization: 2.000 s (separate from extraction). Executed outer Saturate calls: 900; standalone Run calls: 472. Call count is not native iteration count or work share.

| Schedule block | Calls | Wall s | Search + RHS s | Insert/merge s | Rebuild s |
|---|---:|---:|---:|---:|---:|
| `(saturate terms (saturate terms-helpers (saturate terms-helpers-helpers)))` | 23 | 16.374 | 13.884 | 0.092 | 2.142 |
| `(saturate (saturate (saturate type-helpers) type-analysis) (saturate (saturate type-helpers) always-run) error-checking)` | 46 | 14.933 | 13.049 | 0.255 | 1.004 |
| `(saturate context)` | 115 | 3.190 | 0.978 | 0.015 | 2.160 |
| `(saturate (saturate type-helpers) type-analysis)` | 138 | 1.784 | 1.643 | 0.004 | 0.081 |
| `(saturate is-resolved)` | 138 | 1.304 | 1.234 | 0.006 | 0.015 |
| `(saturate subst)` | 115 | 0.885 | 0.812 | 0.010 | 0.034 |
| `loop-iters-analysis` | 23 | 0.229 | 0.220 | 0.000 | 0.000 |
| `(saturate interval-analysis)` | 23 | 0.204 | 0.174 | 0.000 | 0.023 |
| `(saturate ivt-analysis)` | 3 | 0.165 | 0.151 | 0.001 | 0.004 |
| `apply-subst-unions` | 115 | 0.146 | 0.005 | 0.002 | 0.138 |
| `passthrough` | 9 | 0.105 | 0.072 | 0.000 | 0.032 |
| `(saturate mem-simple)` | 23 | 0.077 | 0.060 | 0.000 | 0.014 |

These are measured schedule boundaries, not certified whole-Cell eligibility. Primitive updates, custom merges, subsume and nested ordering still need their own correctness contracts. No claim that every saturated block is directly replaceable by the existing Cell macro. Timers use monotonic wall time; no per-match instrumentation added.
