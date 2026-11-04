# Cell CPU residual Free Join experiment

`cargo cell-bench --lane free-join` runs the ordinary marked program with shared entries, Sieve, and column intersection, then uses a fused Free Join stage for eligible residual table joins. The control lane is `filtered`. This is a Cell-index implementation of a Free Join cover/subatom stage. The independent native reference uses the actual Free Join engine at the immutable egglog-baseline tag. See the [native implementation](../../../../egglog/core-relations/src/free_join/plan.rs) and the [Free Join paper](https://arxiv.org/abs/2301.10841).

At program load, the Cell lane identifies shared slot projections between query atoms. Joins with an existing point plan keep it. When the residual solver chooses a table without a complete key, it can use that table as the cover and probe all remaining table atoms that share a newly bound slot. The cover projects the shared columns; follower indexes probe their own projections plus the columns already bound. An empty child subset rejects the entire cover group before row decoding. A successful probe yields a witness subset that the next stage can consume. Repeated cover prefixes can share probe results. Full functional keys can stream one group per cover row.

The row matcher checks actual canonical values, repeated slots, visibility and the original delta ownership order. Index fingerprints are approximate, including under forced collisions. Proof-recording and local-saturation paths keep their ordinary solver where they need the original observation behavior. This prototype implements the fused residual stage; native materialization, native rule planning and native RHS execution are separate.

```sh
cargo cell-compile --case jacobi-1d --output out/fj-jacobi
cargo cell-bench --program out/fj-jacobi/program.json --lane filtered --repeat 7
cargo cell-bench --program out/fj-jacobi/program.json --lane free-join --repeat 7 --verify-stages
```

## Frozen Pass 3 results

All three columns of timing use the same marked source on one Apple M3 Max thread. The two Cell lanes used the same executable and artifact. One full warm-up per lane preceded seven alternating measured pairs per workload (three for Raytrace). Native timings used a separate release build with its probe counters compiled out, a fresh EGraph per repetition, and the same number of repetitions. These timings exclude compilation, bootstrap, export and audits. The native and Cell engines have different physical stores, action paths and repair implementations. Agreement between the two Cell lanes is tested independently of native endpoint equivalence.

| Case | Cell residual candidates, filtered → Free Join | Native egglog Free Join, ms | Cell filtered, ms | Cell Free Join, ms | Cell FJ faster pairs |
| --- | ---: | ---: | ---: | ---: | ---: |
| fact | 1,552 → 1,482 | 13.66 | 15.35 | 15.33 | 3/7 |
| loopfact | 1,297 → 1,251 | 14.32 | 14.99 | 14.82 | 5/7 |
| sum-bits | 9,309 → 8,816 | 54.78 | 71.96 | 72.03 | 3/7 |
| dot-product | 7,889 → 7,588 | 49.40 | 60.13 | 61.67 | 3/7 |
| sum-divisors | 25,556 → 23,901 | 86.98 | 164.93 | 165.52 | 2/7 |
| fenwick_tree | 26,522 → 25,443 | 123.19 | 196.85 | 191.68 | 6/7 |
| jacobi-1d | 55,754 → 52,188 | 165.50 | 355.88 | 362.28 | 3/7 |
| raytrace | 6,761,462 → 6,601,752 | 21,267.10 | 32,549.74 | 33,123.34 | 0/3 |

## Where the work went

The historical `all_candidate_visits` metric sums direct and residual rows, merge candidates, Spine pivot candidates and entry predicate checks. Keep the additional `counts.free_join` counters alongside it: grouping and index probes are real work even when fewer full rows reach the matcher. `cover_rows` counts rows examined to form group keys, `key_groups` counts prefix groups processed, and `probes` includes misses. `bucket_rows_examined` counts row fingerprints tested for multi-column probes; `bucket_rows_materialized` counts witness references copied when first creating a single-bucket view. `subtrie_uses` counts child domains consumed. These are different operations, with different costs.

| Case | Cover rows | Key groups | Follower probes | Probe bucket rows tested | Rejected cover rows | Graph key lookups avoided |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| fact | 402 | 390 | 465 | 118 | 85 | 174 |
| loopfact | 369 | 369 | 447 | 118 | 49 | 180 |
| sum-bits | 2,133 | 2,088 | 2,471 | 2,477 | 494 | 861 |
| dot-product | 2,636 | 2,614 | 3,126 | 2,065 | 436 | 1,551 |
| sum-divisors | 6,184 | 6,081 | 7,305 | 9,531 | 1,500 | 2,767 |
| fenwick_tree | 7,873 | 7,862 | 9,320 | 4,406 | 1,842 | 3,893 |
| jacobi-1d | 17,881 | 17,849 | 21,193 | 12,899 | 5,115 | 9,350 |
| raytrace | 1,571,042 | 1,566,986 | 1,953,366 | 453,104 | 262,259 | 1,419,459 |

The Cell lane reduces residual row visits across all eight inputs. On Raytrace it performs 939,191 fused stages for 1,571,042 cover rows, about 1.67 rows per stage. Most cover rows require their own projected-key group. Smaller residual search domains leave little repeated work over which to spread projection setup and probes. Keep this lane opt-in. [Raw per-run timings and deterministic counters](cell-fused-results.json)

## Validation scope

The seven smaller tests matched on the initial graph, all 9,971 Run graphs, ordered binding values and schedule fields using the Cargo stage audit. Raytrace matched on the final full graph and schedule/binding counts in every timing repetition. A separate untimed diagnostic build also hashed the ordered RHS inputs at each of Raytrace's 16,883 Runs, including rule, complete slot map, epoch, parent, continuation, known facts, receipts and matched witnesses. Every digest matched. Intermediate full Raytrace graph snapshots were not compared. The triangle regression covers a late insertion and forces every index fingerprint to collide while comparing stages.
