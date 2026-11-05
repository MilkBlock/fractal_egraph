# Run-boundary publication on minimal CPU

The control is the standalone single-thread Cell CPU at `d4074488`. The four arms run the same original Pass 3 schedule with the same frozen guarded 64-block artifacts and context admission. Artifact checksums and individual timings are in [the measurement file](change-batch-results.json).

## What changes

* `bce0cccb` coalesces mutations by table and physical witness as the graph writes and repairs them. Each active table contributes one group to shared entry notification and rule delivery. Ordinary rule wakeups exclude retired witnesses. Shared entry nodes also receive removals, and subsumed facts retain their existing wakeup behavior. The batch owns no decoded rows. Indexes retain their previous keys so they can remove a retired witness. Same-key receipt forwarding still goes directly to Spine. Both ends of a repaired collision appear in the batch.
* `d7678f20` resolves a final row when the table has a live column index, a range axis or a demanded neighbor port. These consumers borrow the same row. Retirement can invalidate a witness without decoding a row. This experiment also offers the row to the next entry refresh when that witness was read in the preceding snapshot. Offers expire at the next publication. The counters distinguish offers from first use.
* `465d815c` retains sharing within a publication. Entry nodes refresh on demand and share a row across actual readers within their existing snapshot cache. There is no speculative handoff across snapshots in this arm.

The compiled entry nodes, rule-to-table dependencies and neighbor port shapes already identify the possible consumers. Subscriptions of the live indexes narrow that set at runtime. This experiment reuses those facts and the existing Bake catalog. Every RHS still commits in its original order. The Run barrier still activates the next consumer, and Spine retains its original ordered producer capture and exact receipt checks.

## Measurement

Four-arm confirmation, medians of complete inference. The paired delta takes the median of per-round differences against minimal. Its value can differ from subtracting the two independent medians.

| Version | sum-bits (ms) | Paired delta (faster / 8) | Raytrace (s) | Paired delta (faster / 4) |
|---|---:|---:|---:|---:|
| Minimal | 33.256 | — | 14.023 | — |
| Shared change headers | 33.313 | +0.299 ms (3/8) | 13.904 | +65.3 ms (1/4) |
| Shared publication plus cross-snapshot row offers | 32.098 | −1.140 ms (8/8) | 14.068 | +44.6 ms (2/4) |
| Shared publication, on demand | 32.269 | −1.092 ms (8/8) | 14.008 | −305.5 ms (3/4) |

In the separate six-round matrix, shared headers took 13.456 s against 13.819 s on Raytrace (6/6 pairs faster) and 31.374 ms against 31.536 ms on sum-bits (5/6). These version rankings varied between matrices. The selected experiment uses on-demand rows. It lowered publication work on both workloads and won all eight sum-bits pairs. The Raytrace end-to-end median improved by only 0.11%, with one pair slower by 232 ms. A general Raytrace throughput improvement remains unconfirmed.

Each timed arm was built with Cargo release defaults, using rustc 1.94.0 on an Apple M3 Max. Each measurement starts a fresh process. Inference covers the original schedule including repair and publication, and excludes program load, graph construction, graph export, audit and offline training. CLI times in the raw data include state export through a pipe into SHA-256. The first matrix used one excluded warm-up and six measured rounds per arm per workload. The four-arm confirmation used one excluded warm-up, eight measured sum-bits rounds and four Raytrace rounds. Arms were rotated and reversed. The two matrices are reported separately. The preliminary row-sharing arm preceded a small change to finalize column versions after a successful update; the confirmation uses the three exact commits above.

| Workload | Minimal → on demand, publication (ms) | Minimal → on demand, delivery + GC (ms) | Header-only arm → on demand, publication row reads |
|---|---:|---:|---:|
| sum-bits | 5.443 → 5.024 | 0.447 → 0.159 | 5,891 → 4,955 |
| Raytrace | 2,856.905 → 2,734.535 | 350.848 → 242.134 | 2,740,549 → 2,428,995 |

Raytrace saved 311,554 publication calls to `Graph::row`. Among changed rows, 57,325 range updates overlapped a column-index update and 254,229 column updates needed only the retired witness. Both cases can be resolved from the consumer subscriptions and change kind. The count measures calls, which can return cached fields. On Raytrace, 330,411 additional headers had no immediate row consumer; on sum-bits there were 1,384. Those header-only counts are not additional savings against the control.

Cross-snapshot offers on Raytrace avoided 42,323 entry row reads (2,697,047 → 2,654,724) after offering 1,232,567 rows. Only 3.43% of offers were consumed. On sum-bits, 251 of 580 were consumed (43.28%). Omitting this cache restores ordinary on-demand entry reads. Compared with retaining offers, the selected arm won 3/4 Raytrace pairs and 3/8 sum-bits pairs. Candidate visits and ordered audit traces agree throughout.

One initial confirmation run accidentally selected the same executable for two labels through a shared Cargo target path. All of its timing samples were discarded. The replacement checked distinct SHA-256 values for all timed executables and asserted version-specific publication and offer counters on every sample.

## Equivalence

For each workload, the baseline audit build (`f10a8b34`) and both forms of publication produce identical ordered per-Run SHA-256 streams for RHS inputs, effective function inserts and value changes, and applied Spine resumptions. The write audit includes merge collisions during repair. The resumption stream includes RHS ordinal, root ID, producer rule, consumer rule and depth. The final graph, original schedule and complete winning Spine prefix tree match the frozen references. A separate small program checks deletion, reinsertion and same-class keys.

| Workload | Runs | Candidate visits | Effective function writes after bootstrap | Resumptions | Maximum depth |
|---|---:|---:|---:|---:|---:|
| sum-bits | 1,428 | 19,154 | 1,450 | 3,034 | 12 |
| Raytrace | 16,883 | 7,263,030 | 1,677,584 | 663,403 | 29 |

## Reproduction

From the selected checkout, generate a program once and use its absolute path for every arm. A clean sum-bits compilation in this worktree differed from the frozen artifact only in the marker-program digest and enclosing artifact checksum. Its executable payload, final graph, ordered RHS, effective writes and resumptions all matched. Use fresh output directories for each CLI run.

```sh
cargo cell-compile --case sum-bits --output out/sum
cargo cell-compile --case raytrace --output out/ray
cargo cell-bench --program out/sum/program.json --repeat 8
cargo cell-bench --program out/ray/program.json --repeat 4
```

Set a distinct Cargo target directory per checkout, for example `export CARGO_TARGET_DIR="$PWD/target"`. For a fresh-process sample, build once in each checkout and execute the binary with the same frozen input. Read `inference_seconds`, `publication` and `entry_schedule.stats` from `report.json`.

```sh
export CARGO_TARGET_DIR="$PWD/target"
cargo build --release --locked -p eggfuse-cell-runtime
"$CARGO_TARGET_DIR/release/eggfuse-cell-runtime" --program "$program" --output "$output"
```

The phase key `query_update` includes Spine, shared entry notification, index maintenance and Sieve. `delivery_gc` includes rule scheduling and garbage collection. For the ordered audit, run the following command in each checkout with the same artifact and a fresh output path.

```sh
cargo run --release --locked -p eggfuse-cell-runtime \
  --features slot-transfer-audit,spine-shapes -- \
  --program "$program" --output "$output"
for stream in rhs writes resumes spine-shapes state; do
  cmp "$baseline/$stream.json" "$output/$stream.json" || exit 1
done
```
