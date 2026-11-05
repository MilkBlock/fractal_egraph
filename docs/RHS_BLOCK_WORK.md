# What a complete RHS actually saves

On Raytrace the frozen broad D3 trial executed 69,732 complete RHS blocks on non-resumed rule applications. Replaying those same applications on the ordinary standalone Cell CPU engine found **49,068 physical frame registrations, zero constructor insertions, zero function updates and zero direct change notifications**. The 570 ordered non-reflexive union requests remained necessary. The ordinary execution already avoided registering frames for the other 20,664 applications.

## Rank by physical work

The audit records the epoch, RHS application ordinal, rule, full input tuple and ordered canonical non-reflexive union pairs. The ordinary and optimized replays validate every event against the frozen artifact. This file selects measurements only. Each trial’s uninstrumented executable runs on the original program without the cohort. The full ranking includes individual rules and the operation sequence after compiler-proved aliases and copies are removed.

| Raytrace RHS shape | Applications | Frame registrations saved by D3 | Distinct intermediate results kept local by the new block |
|---|---:|---:|---:|
| Let | 52,440 | 31,842 | 0 |
| Set | 14,167 | 14,167 | 0 |
| Let → Set | 3,045 | 3,045 | 3,045 |
| Let → Let → Set | 14 | 14 | 28 |
| Let → Let | 66 | 0 | 0 |

Each row saved zero published index updates and zero consumer wakeups. The audit checked two things independently. First, every application in the cohort had the same ordered non-reflexive union pairs and produced no new constructor or function writes. Second, every Run had the same digest of repaired per-table change headers and physical witnesses, the same column/range/neighbor index row counts, reader notifications, shared seed deliveries and effective union counts. Other work in the Run, including repair after a necessary union, can still update indexes and wake consumers.

The D3 route avoided 72,871 canonical lookups on the Raytrace cohort using producer receipts. The new local block still probes for the current canonical key. It saves the physical frame on a full hit without building or maintaining the D3 port collection. A fresh Cell identity is reserved on both paths because subsequent e-class identities depend on its sequence.

## Which intermediates stay in the block

The compiler accepts virtual-constructor Let instructions with an optional terminal constructor Set, after checked static aliases and copies. A typed read of the result of an earlier Let is a local data dependency. Each such result counts once per fully completed block, even if multiple later operands read it. At execution, existing rows supply the values. A non-reflexive Set still requests the original union, with the same operands and relative order. The first missing key promotes the temporary frame, immediately registers the real fact in the graph, and resumes the original suffix after that instruction. Any required Spine publication gets its original key and receipt. Resumed Spine entries keep their original execution path.

For example, Raytrace rule 324 rewrites `AddContext(ctx, Get(c1, index))` to `Get(AddContext(ctx, c1), index)`. The first lookup supplies `v = AddContext(ctx, c1)`. The terminal Set uses `v` in the key of `Get(v, index)` and compares the existing result with the original left-hand expression. In the cohort, **2,086 executions** passed this intermediate value locally. The existing `AddContext` row remains visible to other consumers. Across the cohort, 3,073 distinct intermediate result instances ran locally. Across the entire width-8 execution the count was 219,570 on Raytrace and 1,328 on sum-bits.

A second census followed *new* interior constructor facts produced by admitted width-8 applications, including promoted suffixes. It tracks the actual receipt until the end of the producing Run. A later RHS lookup means a subsequent canonical table lookup that returned that physical witness. This observation does not cover every possible path through a direct receipt.

| At repair | sum-bits | Raytrace |
|---|---:|---:|
| Newly created interior facts | 377 | 46,468 |
| Same physical fact still visible | 306 | 32,494 |
| Forwarded to a visible fact of the same canonical key | 71 | 13,974 |
| Neither visible nor forwarded to a visible fact | 0 | 0 |
| Observed by a later RHS canonical lookup before repair | 59 | 5,394 |
| Forwarded to an older Run's fact with no observed later RHS lookup | 53 | 12,289 |

Every one of these new intermediates occupied an anchor operation of a producing Spine root. That identifies a publication obligation. It does not establish that every anchor won routing or resumed. Among the 12,289 Raytrace candidates, physical registration could only be avoided by supplying a sound same-key forwarding proof early enough, preserving the anchor and its publication order, and covering the other ways a consumer can observe the row. The current trial retains all new physical facts and forwards at the usual repair barrier.

A separate three-input regression exercises a fully reused nested constructor, a prefix hit followed by a miss, and a miss on the first constructor. The Cargo benchmark checks all Run digests against width zero. A tampered compiler plan is rejected; a catalog with no matching operator shape safely admits zero rules.

## Inference and full CLI

| Workload | Inference ms, off / width 1 / width 8 | Complete CLI ms, off / width 1 / width 8 |
|---|---:|---:|
| sum-bits | 33.24 / 33.40 / 32.60 | 136.50 / 141.94 / 141.35 |
| Raytrace | 12,761.62 / 13,045.47 / 12,913.12 | 15,225.24 / 15,668.75 / 15,332.56 |

Raytrace's full-workload frame registrations fell from 2,705,661 to 2,389,486 at width 8. The median RHS phase changed from 3,528.16 to 3,530.21 ms. Every miss still performs the normal insertion, every new fact stays discoverable, and the full-hit path still has to find the existing rows. This matrix found no consistent inference gain.

For held-out admission the rank tool sorts portable sequences of (operation kind, table name) by the actual registrations saved in the training cohort. It takes the smallest ranked prefix accounting for at least 80% of that total, with deterministic ties. The target compiler checks its own typed argument wiring. Each workload selected three signatures. Sum-bits selected Let(ContextOf), Set(Get) and Let(HasArgType); Raytrace selected Let(ContextOf), Set(Get) and Let(ExprIsResolved).

| Train → execute | Target cohort registrations saved | Inference ms, off / all width 8 / selected | Full CLI ms, off / all width 8 / selected |
|---|---:|---:|---:|
| Raytrace → sum-bits | 292 / 366 | 30.77 / 31.31 / 30.93 | 128.78 / 128.81 / 130.26 |
| sum-bits → Raytrace | 39,023 / 49,068 | 12,699.25 / 12,655.87 / 12,651.79 | 15,170.14 / 15,103.84 / 15,084.37 |

Selected versus off was faster in 4/9 sum-bits pairs and 4/6 Raytrace pairs. Median paired differences were +0.04 ms and −47.46 ms, respectively. The sign of the full-width effect also changed between matrices. Neither workload provides a reproducible end-to-end improvement strong enough to enable the feature by default.

The shape experiment and the held-out experiment used separate balanced timing matrices. Each started a fresh process on the same frozen 64-unit program, with nine rounds for sum-bits and six for Raytrace. The CLI clock also includes loading and graph export. Audit, training and compilation ran outside the timings. The timed binaries predate the final per-operand lifetime instrumentation. The final audit replays the same typed decisions and verifies the same graph, writes, scheduling and cohort counters; the executable hashes identify each measurement. The results include raw samples, paired differences, the full per-rule ranking, both catalogs, intermediate lifetimes by rule, and correctness digests. [Machine-readable results](rhs-block-work-results.json).

## Reproduce with Cargo

Use fresh directories. The existing `cargo cell-compile --case sum-bits --output out/sum-bits` and corresponding `raytrace` command create the 64-unit inputs. The following sequence records the original D3 cohort, measures the ordinary and width-8 RHS on the same inputs, and ranks the physical work.

```sh
features=rhs-work-audit,rhs-locals,view-ports,slot-transfer-audit,spine-shapes
for case in sum-bits raytrace; do
  program="out/$case/program.json"
  cargo run --locked --release -p eggfuse-cell-runtime --features "$features" -- \
    --program "$program" --structural-depth 3 --output "out/$case-d3"
  for width in 0 1 8; do
    cargo run --locked --release -p eggfuse-cell-runtime --features "$features" -- \
      --program "$program" --rhs-cohort "out/$case-d3/rhs-cohort.json" \
      --rhs-local-width "$width" --output "out/$case-local-$width"
  done
  cargo run --locked --release -p eggfuse-cell-runtime --features "$features" --example rank_rhs_work -- \
    "$program" "out/$case-local-0/rhs-work.json" "out/$case-local-8/rhs-work.json" \
    "out/$case-rhs-profile.json" >"out/$case-rhs-rank.json"
done

cargo bench --locked -p eggfuse-cell-compiler --features rhs-locals,view-ports --bench cell -- \
  --program out/raytrace/program.json --rhs-local-width 8 \
  --rhs-local-profile out/sum-bits-rhs-profile.json --repeat 5
```

The rank tool also accepts the D3 work file as its comparison input to reproduce the original savings. Remove the profile argument to run all admitted typed shapes at the chosen width. Use width zero as the control. The local RHS path is opt-in.
