# Multi-stage scope for negative recipe evidence

This extends the opt-in standalone CPU experiment in `REFUTATION.md`.
`CELL_CPU_MARKER_REFUTATION=1 CELL_CPU_REFUTATION_SCOPE=multistage` enables it.
The default remains disabled. Neither native egglog nor the main workspace is
changed, and no recipe effects are executed ahead of their original Run.

## Expanded reuse

1. A committed producer frame may enter through an existing, independently
   validated `MultiStage.routes` projection. It no longer needs an explicit
   marker row. The frame must be completed; the original route and source
   binding are still checked by the ordinary executor.
2. Negative evidence can be collected at nested residual-query boundaries,
   with each child retaining its exact read leases and delta restrictions.
   A parent's proof includes the evidence of a reused child. An unsuccessful
   semi-naive partition is never promoted to a complete negative.
3. A failed subprogram can reject a stronger later query: its free variables
   may acquire bindings, aliases, or additional requirements. Repeated-variable
   correlations and fixed values must still agree. Only ordered contiguous
   fragments are considered; there is no arbitrary subset/permutation search.
4. The entry check can use requirements already supplied by local projections,
   as well as unresolved requirements. This allows a failed fragment to span
   previously-read facts and the next unresolved part of a later-stage query.

For example, after proving `P(a,y) AND R(y,z)` has no solution, a later rule
requiring `P(a,b) AND R(b,z) AND Extra(z)` may be rejected if the same proof's read
ranges are still valid. A proof for `a` does not automatically apply to a
different, unconnected identity `a2`. The feature shares evidence across recipe
instances using the relevant common ports; it does not infer symbolic template
closure from one observed failure.

The new multi-stage fixture uses `Seed -> A -> B`, then different `readA` and
`readB` rulesets. The second consumer reuses a previous negative without any
marker rows. A companion fixture inserts the missing boundary fact between
stages; the proof expires and `readB` produces its required result.

## Avoiding a new proof search bottleneck

The first expanded implementation linearly compared certificates with the same
operator sequence. It recorded 2,855 hits (2,821 in later epochs, 300 from another
ruleset), but did 1,590,534 candidate-certificate comparisons. Its paired time
was 232.520 -> 1,020.601 ms. These are retained as a failed intermediate version.

The final index is a discrimination tree over constants, new variables and
repeated variables. Known arguments select constant branches directly; repeated
arguments must respect their earlier bindings. Expired keys are removed from
both the certificate store and the tree. Empty snapshots do not trigger an
unnecessary retirement scan. Read-range accounting is incremental.

One-read failures are not separately materialized: an empty existing index/key
lookup already represents that result. The final run declines 5,991 such
redundant records. This is not a candidate budget or a weakening of matching;
these queries keep their ordinary exact execution.

## Same sum-bits Pass 3, one execution per lane

Same official `source_sk_mk` artifact with the Cartesian Get pair removed, same
release binary and source schedule. `CELL_CPU_MULTISTAGE=1` in both lanes;
the separate observed-composition experiment and Sieve remain off in both.

| Metric | Refutation off | Expanded refutation on |
|---|---:|---:|
| Inference | 240.694 ms | 313.990 ms |
| Search | 124.853 ms | 190.651 ms |
| Index update / proof retirement | 8.572 ms | 13.401 ms |
| Candidate visits | 151,874 | 151,679 |
| Key lookups | 97,113 | 96,941 |
| Peak RSS | 206.59 MiB | 208.34 MiB |

The final prototype remains **30.45% slower**, with only 195 fewer candidate
visits. It has 92 hits, including 77 from later epochs, 29 from other rulesets,
and 35 requiring the stronger-query implication. These categories overlap.
There are 21,613 entry observations through validated continuation routes; the
maximum observed producer continuation depth is 19. This is neither 21,613
distinct reusable Cell families nor a proof of a closed 19-stage region.

The index visits 56,743 discrimination-tree nodes and reaches 35 candidate
certificate leaves for containment validation. These are different units from
the old linear scan's full candidate-certificate comparisons; they do not mean
proof routing costs only 35 operations. 44,923 overall proof lookups and repeated
query-coordinate construction still cost more than the work they eliminate.

There are 579 admitted receipts, 939 delta-incomplete attempts, 420 invalidations,
and 159 surviving receipts / 569 read ranges. Nested certification counters can
overlap; measured engine candidate/key differences above are the work comparison.
No new Join-depth histogram was collected. Continuation depth is not Join depth.

## Validation and outcome

All **1,622** snapshots, including helper stages, have exactly the same facts,
function values, equalities, visibility, full binding sequences and schedule
coordinates with the feature off/on. Thirteen integration tests pass in the
expanded configuration; the original configuration also passes. Five Rust tests
pass, including exhaustive small discrimination-tree containment and removal
checks. Late facts, internal union, hash collisions, function updates, deletion,
finite-stage execution and checkpoint restoration remain covered.

This expands real cross-stage reuse, but is **not enabled or promoted**. The
native original-source baseline remains the previously measured 52.144 ms;
its known graph differences from the standalone Cell executor are not waived.
This work still does not produce an ahead-of-time certificate that an arbitrary
RipSat Cell has no external match. Its input is observed complete negative
queries plus current boundary read leases, with multi-stage recipes supplying
more entries and reusable requirements. More such observations do not by
themselves prove a profitable static closure abstraction.

Reproduce using the existing driver:

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml --bin eggfuse-cell-runtime --example project_stages
CELL_CPU_REFUTATION_SCOPE=multistage EGGFUSE_TEST_FRONTEND=/path/to/eggfuse_frontend python3 -B -m unittest experiments.cell_runtime.refutation_test -v
python3 -B -m experiments.cell_runtime.refutation_bench --pipeline out/marker-shape-sum-bits/remove-compile --output out/NEW --scope multistage --audit
```

Compact results: `refutation-results/multistage-sum-bits.json`. Raw output:
`out/marker-refutation-multistage-indexed-sum-bits/`. No raytrace workload or GPU
benchmark was run. Initialization, compilation/Bake, audits and final export
are outside the reported inference time.
