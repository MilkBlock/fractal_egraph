# Measured contributions

One sweep gives each of the eight frozen inputs one execution. The rows below count consumers that actually applied an RHS with a prior-Run producer owner. All 56 class/target combinations linked. Timings use seven blocked samples per singleton after warm-up, except Raytrace with three. For marginal comparisons we used five blocks per variant, except Raytrace which has one counters-only observation per variant. Do not substitute these statistics for the four pairs from the earlier class-9-only Raytrace run.

| Target | Class 1 | Class 3 | Class 5 | Class 7 | Class 9 | Class 11 | Class 13 | All classes | Library visits saved |
|---|---|---|---|---|---|---|---|---|---|
| fact | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| loopfact | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 2 | 0 |
| sum-bits | 1 | 2 | 3 | 1 | 4 | 4 | 1 | 16 | 0 |
| dot-product | 2 | 1 | 0 | 0 | 0 | 0 | 0 | 3 | 0 |
| sum-divisors | 1 | 2 | 0 | 0 | 0 | 0 | 0 | 3 | 0 |
| fenwick_tree | 2 | 8 | 1 | 1 | 1 | 6 | 4 | 23 | 0 |
| jacobi-1d | 3 | 8 | 2 | 2 | 2 | 7 | 2 | 26 | 0 |
| raytrace | 2 | 71 | 20 | 16 | 24 | 39 | 26 | 198 | 0 |

The full library delivered 271 cross-Run applications and registered 32,873 additional Cells across the eight executions. It saved zero counted candidate visits, key lookups and constructor requests. Seven workloads had repeated joint timings; Raytrace had a work-only joint audit. Both sum-bits and fenwick_tree also passed full-graph and binding comparisons across all 3,550 Run boundaries.

## Why the counts matter

| Class | Native training members | Roots opened | Offered edges | RHS resumed | Live port reads | Visits saved | Lookups saved | Extra registered Cells alone | Extra registered Cells at full library margin |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 1 | 27,399 | 12 | 12 | 12 | 0 | 0 | 10,113 | 10,113 |
| 3 | 1 | 10,118 | 93 | 93 | 186 | 0 | 0 | 29 | 29 |
| 5 | 1 | 29,029 | 291 | 26 | 29 | 0 | 0 | 22,731 | 395 |
| 7 | 1 | 38,342 | 2,720 | 20 | 23 | 0 | 0 | 22,336 | 0 |
| 9 | 2 | 38,331 | 2,720 | 31 | 35 | 0 | 0 | 22,336 | 0 |
| 11 | 2 | 38,306 | 2,720 | 56 | 60 | 0 | 0 | 22,336 | 0 |
| 13 | 1 | 38,329 | 2,720 | 33 | 37 | 0 | 0 | 22,336 | 0 |

The last two columns compare different interventions. Positive extra Cells are a storage cost. Several classes arm the same producer or consumer rules, so adding standalone frame counts overstates the joint cost. Elimination of a frame or lookup is measured against the corresponding control. Opening a root, passing a literal guard and committing a new logical fact are separate events.

## Full library versus the empty portfolio

| Target | Extra cross-Run applications | Extra registered Cells | Lookups saved | Median paired ms saved (faster blocks) |
|---|---:|---:|---:|---:|
| fact | 0 | 51 | 0 | -0.85 (2/5) |
| loopfact | 2 | 42 | 0 | -0.18 (2/5) |
| sum-bits | 16 | 229 | 0 | -0.37 (2/5) |
| dot-product | 3 | 126 | 0 | -0.11 (2/5) |
| sum-divisors | 3 | 655 | 0 | -3.25 (2/5) |
| fenwick_tree | 23 | 613 | 0 | +1.02 (3/5) |
| jacobi-1d | 26 | 1,001 | 0 | -3.79 (2/5) |
| raytrace | 198 | 30,156 | 0 | work audit |

## Exact sum-bits work attribution

All 128 subsets of the seven classes were executed. The exported graph, scheduling fields and binding counts matched. The following signed work attributions add up to the full-library difference. Fractional Cells distribute a shared registration cost across the responsible classes.

| Class | Candidate visits saved | Key lookups saved | Registered Cells saved | Extra cross-Run applications |
|---:|---:|---:|---:|---:|
| 1 | 0.00 | 0.00 | -58.00 | 1.00 |
| 3 | 0.00 | 0.00 | 0.00 | 2.00 |
| 5 | 0.00 | 0.00 | -44.60 | 3.00 |
| 7 | 0.00 | 0.00 | -31.60 | 1.00 |
| 9 | 0.00 | 0.00 | -31.60 | 4.00 |
| 11 | 0.00 | 0.00 | -31.60 | 4.00 |
| 13 | 0.00 | 0.00 | -31.60 | 1.00 |

[Full class-by-workload results and timing samples](results.json) · [All coalition work counters](sum-bits-coalitions.json) · [Training and compatibility controls](evidence.json)
