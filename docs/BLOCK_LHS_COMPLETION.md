# Whole-block postconditions for consumer matching

This opt-in standalone Cell CPU change extends `531a9d74`. The compiler emits
checked constructor postconditions; the engine composes them across the actual
shared phases and lowers consumer reads to guarded ports plus residual matching.
It also uses that projection to prepare eligible consumer RHS inputs before
execution. Enable it with `--block-lhs` in a `two-run-blocks` build.

The existing 64-unit iterative Bake catalogs, markers, schedules and multi-Run
edge catalogs are unchanged. There is no workload-name admission or new training
run. This change uses typed compiler facts that the previous parameter-only
plans did not use to discharge consumer LHS reads.

## What is compiled

`block_postconditions[template]` records each constructor instruction's operator,
normalized key expressions and result expression. Copy definitions and checked
static aliases are normalized. The loader derives the summary again and rejects
an inconsistent certificate. Older frozen artifacts derive the same summary at
load time. Mutable function writes, negative conditions and merge proposals are
not constructor postconditions.

At the first use of a shared shape, the engine composes every retained phase's
input projection into root parameters, literals and earlier result ports. It
indexes the resulting facts by operator and symbolic key once per shape. A
consumer plan then starts with the matched pivot's symbolic fields and computes
a fixed point: any constructor atom with a fully known key supplied by the block
gets candidate ports; its result becomes available to later atoms. Other atoms
remain residual. These shape-specialized plans are cached lazily rather than
enumerating every possible stage combination offline.

For example, the regression fixture executes:

```text
Run 1: Leaf(x)                 -> F(x), Side(x)
Run 2: F(x), Side(x)           -> G(f, side, x)
Run 3: G(f, side, x), F(x), Side(x) -> H(g, f, side, x)
```

The second rule can consume both root facts and share their parameters even
though neither pivot alone contains every RHS input. The third rule anchors on
`G` and reads `F` and `Side` from the earlier phase of the same block. The test
observes a retained three-phase block and verifies that the third-stage plan
contains two reads from phase zero. Inputs are shared before RHS execution, not
attached only after the consumer has finished.

The implementation reuses the register kernel and Spine instruction order.
Provided atoms execute port guards and their original result constraints; they
do not perform ordinary global key matching. Residual atoms execute the original
matcher. It does not yet produce a separate executable that erases every check
for a table-complete plan.

## Runtime obligations

* A positive port must name a current, visible fact of the expected operator and
  actual complete canonical key. Missing, hidden or mismatched ports fall back.
* Constructor operations that reused another fact may have no owned fact port.
  Their result identity can provide an addressing hint. The engine validates
  its defining operator, liveness, visibility and full key before using it.
  An e-class representative is not assumed to identify a unique e-node. Imported
  identities have no encoded Cell address and use the ordinary lookup.
* Bound-result equality checks, primitive guards, function reads and seminaive
  ownership/epochs remain active. Successful Unit guards may supply the Unit
  symbol to later wiring, but the guard still executes.
* An expansion with unbound keys keeps the full original candidate domain. A
  block's local facts are not a proof that no other matching rows exist.
* Matching still completes before ordered RHS execution. RHS leases are Run-local
  and bounded to 65,536 entries. A lease does not bypass the existing complete
  binding equality, source visibility, earlier-Run, free-successor or phase-limit
  checks. Cache keys include the complete projection to distinguish plans with
  different sources for the same consumer input.

## Observed coverage

Both arms use the same frozen program, compact ports, multi-Run completion and
depth limit eight. Only `--block-lhs` changes. Counts are deterministic:

| Metric | sum-bits off / on | Raytrace off / on |
|---|---:|---:|
| Global complete-key lookups | 19,352 / 19,344 | 4,423,086 / 4,420,979 |
| Residual candidate counter | 12,275 / 12,275 | 4,490,637 / 4,490,637 |
| Global constructor requests | 11,502 / 11,502 | 2,064,975 / 2,064,975 |
| Full Cell registrations | 968 / 968 | 319,326 / 319,326 |
| New guarded LHS reads | 0 / 8 | 0 / 2,107 |
| Of those, recovered through result hints | 0 / 0 | 0 / 47 |
| New cross-phase LHS reads | 0 / 0 | 0 / 0 |
| New RHS leases | 0 / 0 | 0 / 0 |

All new workload hits currently come from single retained producers. Multi-phase
matching and new RHS sharing are exercised by fixtures, not observed as new
coverage in these two workloads. Existing Spine resumptions remain 3,034 / 663,403
and longest chains remain 12 / 29. Existing third-phase RHS completions remain
832 on Raytrace. The unchanged residual counter does not count the avoided Spine
point lookups; `provided_atoms` counts actual guarded substitutions separately.

Raytrace visits 373,200 eligible seeds, caches 35,711 dispatches over 187 shapes,
and derives 16,017 positive plans. It performs 30,112 guards to serve 2,107 reads.
Of 28,005 absent own-port result hints, 27,958 are imported identities and 47
succeed. sum-bits uses 389 dispatches over 112 shapes. `full_table_plans` and
`offered_atoms` count potential plans, not successful matches or removed writes.

An early implementation repeatedly scanned all facts for each consumer plan;
one loading template has 27,414 instructions. Indexing each shape once reduced
the diagnostic Raytrace planning time from 88.410 s to 13.286 ms. The final
diagnostic records 1.973 ms inside the port-read guards. These nested timers
exclude much of dispatch/context handling and cannot be added to estimate total
overhead. End-to-end paired measurements are required.

## Paired timing and decision

Five fresh-process pairs follow one excluded warm-up per mode, with alternating
order. Both modes use the same ordinary release binary, one CPU thread, no PGO,
Apple M3 Max and rustc 1.94.0. Loading/export are included in CLI time; offline
training and compilation are excluded. Audit binaries are not used for timing.

| Workload / mode | Inference ms | CLI ms | Peak inference MiB |
|---|---:|---:|---:|
| sum-bits / off | 22.584 | 131.999 | 152.844 |
| sum-bits / on | 23.072 | 132.778 | 153.047 |
| Raytrace / off | 8,386.760 | 11,808.683 | 1,801.969 |
| Raytrace / on | 8,338.495 | 11,735.807 | 1,813.375 |

sum-bits is 2.16% slower by medians and slower in all five pairs. Raytrace is
0.58% faster by medians, but only three pairs improve; differences range from
-222.426 to +498.076 ms. This does not establish a speedup. Raytrace's median
peak RSS increases by 11.406 MiB. The new route stays **off by default**.

[Results and audit digests](block-lhs-results.json) and
[raw timed samples](block-lhs/samples.jsonl) preserve the measurements. Positive
plan counts must not be presented as saved matching or physical work.

The next priorities are to carry exact committed receipts for reused/imported
constructor facts, admit plans where successful port reads can amortize dispatch
and storage, and extend summaries to actual function/merge results with version
guards. That last step needs a different validity contract; an RHS proposal is
not evidence of the value remaining visible after merge. More stage depth alone
does not solve the observed coverage problem.

## Validation

The test matrix passes 24 compiler tests and 28 runtime tests with structural
depth and RHS-work audit enabled. The ordinary feature combination passes 24
and 27 respectively. A default compiler/runtime build also checks successfully.
New fixtures cover whole-block three-Run matching, multi-output closure and RHS
projection, false and true residual guards, bound-result mismatch, deletion,
replacement, union, forged postconditions, non-exhaustive expansion, and result
hints whose union representative has the wrong operator or key.

Both workloads match frozen full-state, every Run's schedule, ordered RHS-input,
effective-write, resume and Spine-shape digests. Forced hash and Sieve collisions
also preserve the sum-bits digests. Only the already-existing artifact-identity
header normalization is applied to the shape stream. These are Cell-versus-Cell
checks; they do not resolve the separate native-egglog visibility discrepancy.

## Reproduction

Use fresh output paths. The multi-Run artifacts below are the frozen inputs from
[the preceding experiment](MULTI_RUN_PORTS.md), including its
checked catalogs. New compiler runs with `two-run-blocks` emit postconditions
automatically; this experiment also verifies backwards loading of those inputs.

```sh
cargo test --release --locked -p eggfuse-cell-runtime -p eggfuse-cell-compiler \
  --features eggfuse-cell-compiler/two-run-survey,eggfuse-cell-compiler/view-ports,eggfuse-cell-runtime/storage-metrics,eggfuse-cell-runtime/rhs-work-audit

# Standalone runtime build, separate from the compiler's serde feature union.
cargo build --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks,view-ports,storage-metrics \
  --bin eggfuse-cell-runtime --example bench_deferred_incidence
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/lhs-pairs \
  out/multi-sum.json out/multi-ray.json 5 --block-lhs

cargo build --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks,view-ports,storage-metrics,slot-transfer-audit,spine-shapes \
  --bin eggfuse-cell-runtime --example bench_deferred_incidence
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/lhs-audits \
  out/multi-sum.json out/multi-ray.json 0 --audit-block-lhs

CELL_CPU_HASH_BITS=0 CELL_CPU_SIEVE_COLLISIONS=1 \
  target/release/eggfuse-cell-runtime --program out/multi-sum.json \
  --output out/lhs-collisions --multi-run-blocks --block-lhs

# Small fixture comparison through the Cargo compiler/bench entry point.
cargo run --release --locked -p eggfuse-cell-compiler \
  --features two-run-blocks,view-ports -- bench \
  --program out/fixture/program.json --reference-program out/fixture/program.json \
  --multi-run-blocks on --block-lhs on --repeat 5
```
