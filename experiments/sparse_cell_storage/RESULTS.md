# Sparse Cell allocation: unchanged Bake/workload

Apple M3 Max; each timing variant runs once, RAYON_NUM_THREADS=1, no profiler
or frontier audit during timing. Actual GPU matching + native RHS/merge/rebuild,
not a standalone storage prototype or a full eggcc benchmark. Dense control is
the pre-change f120fba executable/worker, frozen before editing; sparse uses the
same four training contexts, five library variants, eight consumers, 512
instances per structural family, facts, schedule and union/subsume perturbations.

## Resident memory and the two previously reported inference phases

All arrows are dense → sparse. Memory is the sum of live persistent Metal buffer
allocations after the disrupted phase: **not RSS or transient allocation peak**.

| Library | Interface plans (unchanged) | GPU MiB | Intact inference ms | Disrupted inference ms |
|---|---:|---:|---:|---:|
| none | 0 | 25.94 → 25.94 | 7.506 → 5.688 | 5.154 → 4.628 |
| single | 2 | 33.69 → 19.10 | 6.340 → 5.774 | 6.646 → 8.271 |
| repeated_single | 2 | 33.69 → 19.10 | 8.145 → 6.293 | 6.581 → 7.321 |
| base_cells | 7 | 119.19 → 21.19 | 8.020 → 6.226 | 8.636 → 6.875 |
| composed_cells | 24 | 729.45 → 40.45 | 14.818 → 6.345 | 41.160 → 7.994 |

The rich library retains **4 native-certified states / 24 interface plans**.
Persistent allocations fall 94.46% (18.03× smaller), with no smaller library,
sampling, candidate budget or truncated output. Intact and disrupted phase
times improve by 2.34× and 5.15× in this run. Small-library disrupted timings do
not all improve; sizing/compaction introduces synchronization and dispatches.
These single-run timings are not statistical speedup estimates.

## Bootstrap and total inference must also be reported

Initialization includes worker startup/compiler/cache effects; it is excluded
from the original two inference phases. Bootstrap is the first consume pass.
Total inference = bootstrap + intact + disrupted, excludes initialization,
Bake, input parsing/insertion and final comparison.

| Library | Initialization ms | Bootstrap ms | Total inference ms |
|---|---:|---:|---:|
| none | 420.641 → 420.647 | 17.540 → 16.450 | 30.200 → 26.766 |
| single | 181.087 → 54.211 | 14.984 → 16.015 | 27.970 → 30.059 |
| repeated_single | 44.049 → 43.441 | 14.996 → 17.461 | 29.722 → 31.075 |
| base_cells | 167.861 → 51.126 | 16.208 → 41.587 | 32.864 → 54.688 |
| composed_cells | 168.120 → 50.388 | 28.441 → 79.099 | 84.420 → 93.439 |

**Total rich-library inference does not improve in this single measurement.**
Sparse bootstrap contains 56.724 ms in Rust binding decoding/sorting versus
0.042 ms dense, with the same binding counts and bytes. Base-library decoding
also spikes. That CPU path was not changed, and the cause has not been isolated;
do not discard the measurement or claim an end-to-end speedup. Rich bootstrap
worker round trip itself drops from 27.740 to 15.639 ms. Shader initialization
times vary strongly too, so they are not evidence of the allocator's speed.

## Rich-library storage accounting

| Item | Dense | Sparse |
|---|---:|---:|
| Native row slots / capacity | 20,096 / 32,768 | 20,096 / 32,768 |
| Interface frame capacity | 786,432 (rows × 24) | 65,536 |
| Active interface instances | not recorded | 54,784 |
| Historical slot high water / currently free | not recorded | 55,296 / 512 |
| Slots reused across events | not recorded | 1,920 |
| Interface frame bytes | 88,080,384 | 7,340,032 |
| Recorded watch entries / capacity | 88,960 / 16,777,216 | 82,560 / 131,072 |
| Recorded ready-port entries / capacity | 17,920 / 4,194,304 | 16,256 / 16,384 |
| Directory + metadata + free-list bytes | no directory | 1,179,688 |
| Watch log + hash-head bytes | 637,534,208 | 5,767,168 |
| Ready-port bytes | 33,554,432 | 131,072 |

An interface instance is an actual anchor-row/projection binding, **not** a
distinct baked semantic Cell. Ready, waiting-for-a-key and uncertified shapes
all retain state; missing-key subscriptions are necessary for completeness.
Several interfaces/anchors can refer to the same native structure. Logs can
contain invalid old versions until compaction; differing log counts do not
mean bindings were discarded. Capacity rounding and a reusable high-water pool
remain, but are determined by observed instances/dependencies, never rows ×
all plans. This is an allocation fix, not a claim of compressing native e-nodes.

## Semantics and work

- All five libraries: exact complete native facts, values, equality and visibility
  agree after both phases. Timing runs check final graphs outside timed regions.
- Separate 32/family check run audits every incremental binding frontier before
  native RHS. All pass, including unknown/ambiguous contexts falling back.
- 128/family profile exactly preserves previous Ready/Suspected/Unmapped counts,
  successful bindings, reused reads and query/locator probes. Rich intact Ready
  stays 1,024/1,024; disrupted stays 832/1,600. Nothing is removed from Bake.
- 31 GPU/native regressions pass. New tests keep Cell/watch/port capacities
  constant after 4,096 unrelated rows, reject wrong literal anchor indices,
  cover cache-only mode, and recycle slots through late facts and same-batch
  retire/reallocate with exact non-idempotent RHS counts.

Raw JSON includes all timing phases and counters. Run `python3 compare.py` to
recheck comparability and regenerate this table; no benchmark rerun is needed.
