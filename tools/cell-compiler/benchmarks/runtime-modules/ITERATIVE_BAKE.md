# Iterative Bake of bounded, scheduled Cell CPU blocks

The input is a validated warm-start artifact containing a spine-block catalog. Round zero runs the unextended catalog. Each learning round restarts the original program with a frozen catalog, tracks successful applications, selects a bounded batch of continuations and appends stable IDs. A final execution verifies the last installed batch. The driver requires the original per-Run ordered RHS input digests, schedule and final logical graph to stay equal before publishing a deployable artifact.

## Limits and admission

Native CS discovery defaults to a maximum expanded depth of 4, 8 distinct member applications and 64 units. A primitive counts as one. The observed DAG checks its parent depth and bounds the membership traversal before preparation. If a prefix no longer fits, the current mutation becomes a fresh root with its external input obligations. History windows compute depth from the real producer edges inside their members. Legacy layer discovery also enforces the limits. When the native unit budget is full, the source schedule continues but CS stops adding units. Native early admission is chronological. The iterative CPU trainer observes the entire run before choosing its next batch.

The iterative catalog starts with the seed IDs and adds at most 8 new IDs per round, up to 64 total by default. Both depth and node bounds apply to the fully expanded primitive path. For this first linear implementation they coincide. A four-rule chain cannot be smuggled through a depth-two budget by giving one side a macro ID.

On each scheduled, checked anchor, the executor identifies the consumer leaf and reduces adjacent completed segments on the cursor using the frozen binary programs. The key contains the left unit ID, publishing operation and right unit ID. Only the winning applied origin contributes frequency and a witnessed producer/consumer Run pair. Forks have independent segment stacks. Missing entries clear the segment history. A missing port or losing binding discards only that candidate. The original RHS, repair/publication order, mutable ports and intermediate facts remain visible at their original boundaries.

A fully covered, matched macro delivers the selected cursor directly with its sealed RHS binding. It moves the deduplication key into the answer and avoids reconstructing a second key from the binding slots to recover the origin on application. Partial coverage retains the ordinary origin lookup. Frequency alone grants no benefit credit. Ranking uses the observable origin-key slot copies eligible for elimination, followed by support and deterministic IDs. A right operand already using this fast path earns no duplicate credit. The default candidate table holds 512 shapes with minimum support 2. At capacity, decrement-and-prune counters give conservative admission estimates. The round stops on insufficient profitable evidence, full catalog or round limit.

Loading checks the seed and recipe hashes, earlier parent IDs, the producer output table against the right entry, the expanded bounds and duplicate normalized paths. The driver registers the validated programs with the native Cell e-graph's parent-linked constructor before executing each frozen catalog. Finite block certification covers dispatch and the binding handoff. It grants no permission to omit a future Run or substitute a saturated endpoint.

## Real Pass 3 executions

| Workload | Warm-start leaves | Depth 2 / 3 / 4 blocks in final catalog | Depth-four completions in final run | Direct RHS origins | Origin-key slot copies avoided | Answer slot copies avoided |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 32 | 15 / 9 / 8 | 146 | 575 | 1,666 | 1,723 |
| Raytrace | 32 | 15 / 8 / 9 | 12,458 | 61,407 | 178,932 | 180,675 |

Both started from the previous study's frozen 32-block artifact for that workload. The training and final replay in this study use the same workload. Round zero observed 33 distinct adjacent shapes on sum-bits and 37 on Raytrace. The catalog sizes were 32, 40, 48, 56 and 64. On both inputs block 48 expands to rule entries `143 → 55 → 143 → 137`. It completed 33 times on sum-bits and 4,934 times on Raytrace in the final run. These are four separately scheduled activations. The two copy counters refer to different operations. The answer-side count includes candidates that participate in the ordered deduplication step. No throughput or candidate-visit speedup is inferred from them.

All five executions per workload agreed on the schedule and final graph and on all 1,428 and 16,883 ordered per-Run RHS digests respectively. Each final digest vector also matched its previously frozen 32-block reference byte for byte. The training driver compares streaming graph exports within one build. The compiler and standalone runtime can serialize JSON object keys in different orders through Cargo feature unification. Standalone deployment is checked against the standalone frozen reference. [Round evidence, actual parent block IDs, sample Runs, work and digests](iterative-bake-results.json)

## Pure Cargo reproduction

Use a fresh output directory. Set `seed` to the frozen 32-block artifact, or to a validated spine-block artifact produced using the [consumer-block workflow](SPINE_SHAPES.md).

```sh
export CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1
export CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_JOIN_INTERSECTION=1 CELL_CPU_SIEVE=1
export CELL_CPU_FREE_JOIN=planned CELL_CPU_KNOWN_RESULTS=static CELL_CPU_RANGE_GUARDS=1
export CELL_CPU_SLOT_TRANSFER=off CELL_CPU_FD_KEYS=0 CELL_CPU_SCALAR_MERGE=0
cargo cell-iterate --program "$seed" --output out/iterative-sum --rounds 4 \
  --max-depth 4 --max-nodes 8 --new-per-round 8 --max-units 64 \
  --candidate-slots 512 --min-support 2
cargo run --release --locked --manifest-path engine/cell-runtime/Cargo.toml \
  --features iterative-bake -- --program out/iterative-sum/program.json --output out/replay
```

`rounds.json` records evidence for the next batch and audit hashes. Each `round-N` directory contains the actual frozen catalog and its Cell e-graph. `program.json` is the final executable artifact. `expanded.json` maps each ID to original rule/pivot entries and output-operation links. `rhs.json` contains the final ordered per-Run digests. For native Bake, the corresponding limits are `EGG_LAYOUT_BAKE_MAX_DEPTH`, `EGG_LAYOUT_BAKE_MAX_NODES` and `EGG_LAYOUT_BAKE_MAX_UNITS`. The iterative CLI inherits those defaults and permits explicit command-line overrides.
