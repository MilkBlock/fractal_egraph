# Incremental semijoins at planned Free Join entries

The control is the standalone single-thread Cell CPU on the change-batch branch (`df279cc4`). The compiler derives a necessary guard from a typed seminaive LHS and records all rule/pivot consumers of the same driver table and first Free Join projection. The loader independently derives the guard map and rejects a different emitted plan. It derives missing guards when loading older programs. The shape accepts one to four projected target columns, possibly including stable literals. Self-joins and representative-dependent literals stay on the existing route. Workload, table and rule names are absent from the admission logic.

The frozen Bake catalog, scheduled block switching and ordered RHS are common to all arms. Existing reverse Free Join takes priority. A pivot with scheduled Spine cursors retains its original routing. Sieve, exact matching and semi-naive witness ownership still run for surviving entries. An absent fingerprint tuple proves an empty exact domain. A present tuple is a candidate and can be a hash collision.

## Survey

For every observed rule/pivot/projection, R counts accesses reaching that projection after earlier admission decisions. K sums distinct selector keys per Run/rule/pivot, and Z counts accesses with a nonempty projected witness set before the owner test. A driver may reach several projections. The separate cross-version survey compares the complete ordered witness set of the same projection key with its last observed version, including negative results. A shared key is charged to its first revisiting consumer. These counters come from a diagnostic-only build, excluded from timing.

| Frozen pass | R | K | Z | Cross-version revisits | Same witness membership | Still empty |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 24,535 | 19,860 | 11,345 | 5,114 | 4,229 | 2,158 |
| Raytrace | 7,326,574 | 5,966,146 | 3,570,542 | 1,019,902 | 888,794 | 498,794 |

One Raytrace pivot shared a necessary `BodyContainsExpr` to `DoWhile` projection across 13 compiled consumers. For several of its rules the survey measured R = 66,613, K = 6,767 and Z = 17,646 per rule, summed across Runs. There are also low-reuse pivots. The per-projection table includes zero-visit projections. Per-rule/pivot records are in the results JSON.

## Algorithms and costs

* **Persistent frontier.** Pool observations of the checked first projection across its consumers. After 512 observed rows, admit a guard when `2 * (K + Z) < R`. Unsuccessful samples back off. Warm states start on a residual frontier of at least 32 rows, capped at eight guards and 65,536 combined table witnesses per guard. The target stores support counts by tuple and the previous tuple for each witness. The driver stores eligible witnesses ordered by (epoch, witness). After repair and column-index publication, process old/new target memberships together, visit driver buckets only on net zero/nonzero transitions, and refresh changed driver witnesses. Additional target witnesses at an already supported key still run through the original target pivot. An ordered range lookup starts at the consumer watermark. Bound the speculative scan at half the input delta, with ordinary entry fallback. For an admitted sparse frontier the range traversal costs O(log N + V), where V includes visited table-wide eligible witnesses before the shared-node predicate. Maintenance additionally pays for changed driver/target headers, support flips, affected driver buckets and tree updates.
* **Key-specific validity.** On a target-version change, validate a cached probe against the last membership version of its smallest selector bucket. An absent bucket is a valid negative certificate while absent. A row changing another projected column dirties retained columns as well. This is necessary for compound keys. A missing, moved or changed bucket triggers a reprobe. The cache stores a version and a bucket identifier, and the existing exact row verification and the per-access ownership check stay in place. This avoids re-materializing unchanged witness sets, but the bucket check itself costs a lookup and columns must maintain versions.
* **Run-local sharing.** A separate arm uses only statically shared first projections with at least three consumers. After a second real request for the same (node, watermark, projection) within a Run, group driver rows by key and test support once per distinct key. Reuse the resulting ordered subset across remaining consumers. Bound the scan, stop dense or high-cardinality groups and cap retained witnesses at 65,536. This trades persistent update work for re-scanning on subsequent Runs. For C identical frontiers of R rows, K keys and Z supported rows, a successfully shared build takes expected O(R + K probes + C Z) entry work for fixed projection arity. A compound probe also pays for the scanned target bucket. The initial consumer and failed builds are included in measured timings.

## Measurements

Timing uses release CLI processes on an M3 Max with rustc 1.94.0, identical frozen artifacts and original schedules. Inference includes RHS, repair, index maintenance and delivery. Parsing, graph export, audit and offline Bake are outside the inference timer. In each matrix, each arm had one excluded warm-up. Arm order rotated by round and reversed on alternating groups. There were eight measured rounds per arm on sum-bits and four on Raytrace. No diagnostic sampling or fine-grained timers were enabled in the timed binaries. Runtime admission of the persistent guard is part of its timed cost.

Four-arm matrix. Times are independent inference medians. Paired differences use each arm's corresponding control round.

| Mode | sum-bits ms | Paired delta ms, faster pairs | Raytrace s | Paired delta ms, faster pairs | Raytrace entry visits | Raytrace full probes |
|---|---:|---:|---:|---:|---:|---:|
| Off | 33.005 | — | 13.261 | — | 6,790,332 | 2,769,180 |
| Persistent frontier | 33.885 | +1.147, 2/8 | 13.987 | +156.801, 1/4 | 6,315,210 | 2,699,361 |
| Key-specific validity | 33.754 | +1.087, 2/8 | 13.753 | +355.220, 1/4 | 6,790,332 | 2,222,717 |
| Both | 34.517 | +2.534, 2/8 | 13.523 | +108.743, 1/4 | 6,315,210 | 2,157,092 |

The persistent frontier on Raytrace delivered 129,438 out of 644,388 source entries in 1,365 accepted batches. It skipped 514,950 before Sieve. The net reduction at Free Join admission was 475,122. Comparing the two arms, 405,303 fewer cache hits and only 69,819 fewer full probes account for that difference. The shared loop projection contributed 367,934 skipped entries. A separate compound projection contributed 85,472. Building nine states (one eviction) visited 122,852 witnesses. Maintenance processed 378,849 driver and 31,271 target headers. It saw 30,625 support transitions, with 43 driver matches enumerated from their affected buckets. A separate instrumented run measured 10.35 ms of initial builds and 32.14 ms in the persistent publication callback. This timer excludes trial sampling, entry routing and range traversal. The total search and publication medians (off → frontier) were 5,871 → 6,208 ms and 2,563 → 2,751 ms. These phase differences cannot be attributed solely to the callback.

With both mechanisms, 612,088 fewer full probes led to 65,893 fewer target bucket rows examined. Materialization fell by just 6,623 of 3,468,021 witnesses. The persistent guard stayed cold on sum-bits. Key-specific validity cut its probes from 13,966 to 11,139. All new experiment modes remain opt-in.

The separate Run-local matrix compared its own off/shared binaries. On sum-bits the medians were 31.603 / 32.801 ms (+1.280 ms paired, 3/8 faster). On Raytrace they were 12.335 / 12.496 s (+191.173 ms paired, 1/4 faster). Run-local sharing lowered Raytrace Free Join admission from 6,790,332 to 6,438,945. It built 140 frontiers, reused them 590 times and stopped 894 builds early. Establishing the supports examined 271,647 source rows and 126,688 distinct keys across trials. It saved only 23 subsequent full probes.

The historical candidate metric stayed at 19,154 on sum-bits and 7,263,030 on Raytrace in every arm. It covers matching and entry predicate checks, while the admission, stamp and maintenance work measured here is reported separately.

## Correctness

Each mode reproduces the frozen final graph, schedule and full winning Spine prefix tree. Relative to the independent minimal audit reference, every ordered per-Run RHS input, effective function write and applied resumption agrees. Sum-bits has 1,428 Runs, 1,450 effective writes and 3,034 resumptions at maximum depth 12. Raytrace has 16,883 Runs, 1,677,584 effective writes and 663,403 resumptions at depth 29. The combined persistent and stamp path also matched its control on sum-bits with zero-bit bucket fingerprints. Dedicated tests cover compound selectors, retire/reinsert, multiple supports, a same-key replacement, union repair, epoch ordering, stale negatives, and missing or corrupted compiled guard metadata. Cargo runtime and compiler suites passed.

## Reproduction

From a fresh output directory, compile each named program once. Use the same program for every arm. A survey build emits `projection_survey` in `report.json`. The ordinary entry and probe counters are under `free_join`. The new maintenance and frontier counters are under `semijoin`.

```sh
mkdir -p out
cargo cell-compile --case raytrace --output out/ray
program="$PWD/out/ray/program.json"
cargo run --release --locked -p eggfuse-cell-runtime --features projection-survey -- \
  --program "$program" --output out/survey
cargo run --release --locked -p eggfuse-cell-runtime -- \
  --program "$program" --output out/off
for features in projection-semijoin projection-stamps projection-semijoin,projection-stamps projection-shared; do
  cargo run --release --locked -p eggfuse-cell-runtime --features "$features" -- \
    --program "$program" --output "out/$features"
done
```

Use `--features slot-transfer-audit,spine-shapes` in the control and add those two features to each experimental audit. Compare `rhs.json`, `writes.json`, `resumes.json`, `state.json` and the winning prefix tree. Use `CELL_CPU_PROFILE=1` in a separate process for diagnostic build and maintenance time.

[Per-projection counters](semijoin-projections.tsv) · [Paired samples, per-route counters and audit digests](semijoin-results.json)
