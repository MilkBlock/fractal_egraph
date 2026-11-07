# Executing a consumer from a frozen Bake unit

This opt-in standalone Cell CPU experiment starts at `b0fbe5e0`. It uses the two selected [iterative Bake checkpoints](ITERATIVE_LHS_BAKE.md). Raytrace unit 288 and sum-bits unit 289 encode the same path. Rule 57 publishes its constructor at instruction 3 and resumes rule 57 through pivot 2.

## Consumer execution

At configuration the executor lowers the selected composition's typed terminal. The admitted shape has copies, checked integer additions available from the LHS, and one terminal constructor. The compiler maps RHS arguments to positions in the sealed query tuple. It verifies both operands and the integer result sort, accepting reversed operands for checked binary integer addition. The unit is chosen by catalog-local ID. The lowering uses typed operations and slot identities.

On this unit, the ordinary matcher already checks `i + 1 < len`, using the current length on the scheduled snapshot. The sealed tuple contains `tuple` at position 0 and the successful result of `i + 1` at position 5. The RHS can therefore submit `Get(ports[0], ports[5])` at its original application ordinal. Matching, mixed-entry deduplication and seminaive ownership use the original ordered binding. The execution route consumes its flat values directly. Stage snapshot and auxiliary analysis clients can request a named shadow map. The frozen stream audit serializes the actual flat ports and required slot IDs directly.

The constructor checks its canonical key at commit time, after any preceding RHS effects. A valid typed donor goes through the existing live-receipt and complete-key checks. Otherwise the executor makes the ordinary keyed request. Reuse reserves the original logical Cell ID. A birth puts the sealed values and required computed slots into a shared-layout port record and runs the existing activation, dependency, index and publication code. The result uses the original Cell ID and destination slot 10. The record retains the seven query slots and computed slot 9, so later reads, repair and retirement can use the original template. Slab capacity or disabled compact storage selects the ordinary frame representation.

## Fixed-cohort results

| Workload | Applied | Prior LHS reads served by this unit | Existing successors | New successors | Full Cell registrations, control → switched |
|---|---:|---:|---:|---:|---:|
| sum-bits | 19 | 4 | 0 | 19 | 968 → 949 |
| Raytrace | 2,389 | 260 | 43 | 2,346 | 319,326 → 316,980 |

On Raytrace the control rebuilt 2,389 binding maps at match time. The switched route consumed 2,389 sealed vectors, reused 2,389 checked additions and created 2,346 compact records directly. Both arms issued 2,389 constructor requests in the cohort. All 43 existing successors were found by keyed lookup. The source anchor supplies the current index, while the target key uses the next index.

Raytrace ended with 300,469 / 298,157 live full Cells and 687,913 total producer records in each arm. The full-registration count in the table is cumulative. Both executions created 820,439 logical facts. This measures a representation change for observable births. Sum-bits ended with 2,875 total producer records in each arm.

## Verification

The diagnostic build compares the selected Run/application ordinals and birth outcomes. It also hashes the actual constructor Cell ID, instruction, table, canonical key and returned witness at each ordinal on both code paths. All 2,389 / 19 constructor events matched. Both workloads also passed frozen full graph, schedule, per-Run ordered RHS inputs, effective writes, resume stream and Spine-shape checks. The shape-header comparison uses the frozen source program identity. Raytrace had 663,403 resumptions with longest chain 29, and sum-bits had 3,034 with longest chain 12.

A renamed rule fixture checks union, deletion, a changed function bound, compact-record retirement and ordinary storage fallback through the executor. It also rejects a leaf as a composition and an RHS addition whose constant differs from the matched LHS calculation.

[Machine-readable audits](baked-consumer/audits.json)

The sum-bits forced hash and Sieve collision audit also matched all frozen graph and stream digests, including the selected unit’s count and effect digest. The feature test matrix passed 29 compiler and 29 runtime tests.

## Release timings

Five fresh-process pairs followed an excluded warm-up on Apple M3 Max with one CPU thread, no PGO and the same binary in each arm. The control surveys the selected cohort and executes the ordinary RHS. The switched arm enables the compiled terminal. The inner clock covers selected RHS applications, including their commit work. Search-side sealing is charged to whole inference. Peak RSS is sampled through inference.

| Workload | Selected RHS total, control → switched, ms | Inference, control → switched, ms | Full CLI, control → switched, ms |
|---|---:|---:|---:|
| sum-bits | 0.015 → 0.018 | 23.300 → 23.274 | 137.355 → 135.707 |
| Raytrace | 2.634 → 2.690 | 8,154.700 → 8,143.652 | 11,510.360 → 11,481.671 |

Three of five Raytrace pairs improved. Paired differences spanned −82.016 to +57.290 ms. Median peak RSS rose from 1807.922 to 1826.156 MiB. The observed full-inference difference falls within the paired variation. The experiment stays opt-in. [Release samples](baked-consumer/samples.jsonl) · [Measurement summary](baked-consumer/results.json)

## Cargo reproduction

Use fresh output directories. Generate the `lhs-sum`, `lhs-ray` and multi-Run base artifacts with the [iterative Bake commands](ITERATIVE_LHS_BAKE.md#cargo-reproduction). Build the runtime independently of the compiler to reproduce the frozen audit serialization.

```sh
cargo build --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks,view-ports,storage-metrics \
  --bin eggfuse-cell-runtime --example bench_deferred_incidence

target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/consumer-pairs \
  out/lhs-sum/program.json out/lhs-ray/program.json 5 --baked-consumer \
  out/multi-sum.json out/multi-ray.json 289 288

cargo build --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks,view-ports,storage-metrics,slot-transfer-audit,spine-shapes \
  --bin eggfuse-cell-runtime --example bench_deferred_incidence
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/consumer-audits \
  out/lhs-sum/program.json out/lhs-ray/program.json 0 --audit-baked-consumer \
  out/multi-sum.json out/multi-ray.json 289 288
```

For a single execution, pass `--multi-run-blocks --baked-consumer 288` on the Raytrace checkpoint to measure the cohort. Add `--execute-baked-consumer` to switch its RHS.
