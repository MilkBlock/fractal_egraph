# Bake quality: fixed workload ablation

Actual GPU/native execution. This uses synthetic held-out bindings for four tuple families, not whole eggcc.

Profile: 128 instances per family, eight consumers. All libraries see identical facts and schedules.

| Library | Training contexts / distinct states | Ready seeds | Located Cell atoms / all LHS atoms | Actual internal reads reused | Join + locator probes |
|---|---:|---:|---:|---:|---:|
| none | 0 / 0 | 0/1,024 (0.0%) | 0.0% | 0 | 6,656 |
| single | 1 / 1 | 768/1,024 (75.0%) | 23.1% | 768 | 5,888 |
| repeated_single | 8 / 1 | 768/1,024 (75.0%) | 23.1% | 768 | 5,888 |
| base_cells | 2 / 2 | 1,024/1,024 (100.0%) | 30.8% | 1,792 | 4,864 |
| composed_cells | 4 / 4 | 1,024/1,024 (100.0%) | 53.8% | 2,560 | 4,096 |

Repeated Single training stays at 75%. Single appears at root or non-root positions in three families; adding Pair covers the fourth. Composed contexts do not increase the already 100% Ready rate, but make one context cover more atoms. The three external relation atoms always remain outside training: **entire-LHS-in-one-Cell coverage is 0% in this design**, even when Ready is 100%.

Counting locator cost, the composed library reduces query index probes from 6,656 to 4,096 (38.5%). Average probes per valid seed: 6.50 to 4.00. These are index-lookup work counts, not a wall-clock speedup or storage compression ratio.

| After boundary changes | Ready / all valid seeds | Ready share of complete bindings | Suspected seeds | Proven missing-next seeds |
|---|---:|---:|---:|---:|
| none | 0/1,600 (0.0%) | 0.0% | 0 | 0 |
| single | 640/1,600 (40.0%) | 71.4% | 352 | 256 |
| repeated_single | 640/1,600 (40.0%) | 71.4% | 352 | 256 |
| base_cells | 832/1,600 (52.0%) | 92.9% | 704 | 64 |
| composed_cells | 832/1,600 (52.0%) | 92.9% | 704 | 64 |

The richer catalog cannot make missing facts exist or make a non-unique location unique. With the composed library, Ready work still produces 92.9% of successful complete bindings, but Ready is only 52% of all valid seeds. These are different denominators.

Old-workload control: replacing the old library with all four new states leaves Ready at **7,168/10,240 = 70%**, with the same reuse/covered-atom/probe counts. Its shortfall was not missing Single/Get training.

Timing: 512 instances per family, one run each, no profiling/audit.

| Library | Bake ms | Setup ms | Intact inference ms | Disrupted inference ms | Live GPU MiB | Projection plans |
|---|---:|---:|---:|---:|---:|---:|
| none | 0.00 | 43.57 | 7.326 | 5.643 | 25.94 | 0 |
| single | 1.36 | 62.89 | 9.045 | 6.671 | 33.69 | 2 |
| repeated_single | 4.84 | 54.38 | 7.491 | 6.305 | 33.69 | 2 |
| base_cells | 1.92 | 45.96 | 9.689 | 8.490 | 119.19 | 7 |
| composed_cells | 3.57 | 50.39 | 47.470 | 28.939 | 729.45 | 24 |

High coverage is not sufficient for current-runtime speed. Four states compile into 24 projection plans. The rich run has 88,960 recorded watch entries but reserves 16,777,216. `FamilyExecutor.advance` sizes Cell storage by row capacity × projection count and watch storage additionally by maximum prefix length. This directly explains the storage over-reservation. We did not change the runtime between libraries.

In the rich intact run, 37.727 ms of 47.470 ms is inside the worker round trip, including only 1.092 ms of measured GPU command time. Host allocation/copy/dispatch/synchronization is a likely contributor, but those host subphases were not separately timed; this is not proof that allocation alone dominates wall time.

This supports improving Bake **structural diversity and composition coverage**, not simply producing more examples. It also identifies the next implementation issue: sparse allocation for actual Cell/interface instances and their watches, rather than the full row/projection product.

All five libraries agree with complete native facts/equality/visibility at both phases. Separate check-mode frontier audits pass. Native whole-Cell/export tests: 11 passed.
