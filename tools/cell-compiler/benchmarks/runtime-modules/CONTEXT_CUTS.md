# Discovering reusable contexts across Pass 3 rules

The Rust compiler treats each LHS table atom as an edge connecting its typed slots. It tries each table as the root and each other table as the terminal. Between them it constructs a connected prefix, choosing a read with the fewest unbound key slots. A primitive executes as soon as its inputs are bound. Any remaining primitives must be executable after the terminal. A valid cut covers the entire LHS and shares at least one slot with the terminal. The compiler keeps one traversal per root, ranked by total unbound key slots in the prefix, number of partial reads, and terminal boundary width. This search covers seminaive queries with two to eight tables. Disconnected prefixes and unschedulable primitives fall back. The artifact stores atom orders. The loader checks these and derives slot projections from the typed recipe.

This search finds the original `DoWhile` prefix independently. Leaving `Const` at the terminal keeps its unknown literal and type out of the retained prefix. The other orientations also include two-table joins, chains of inverse reads, repeated relations and numerical guards. No rule IDs or operator names steer extraction.

## Admission from the data

At a legal Run the engine selects among compiled roots by current root and terminal sizes. It caps a root table at 256 rows, a context at 4,096 outputs plus read leases, and port candidate work for a general prefix refresh at 4,096 visits. An oversized orientation can retry after its input tables shrink. Stable groups either stream the terminal delta or probe their own keys into the shared delta when the indexed domains are smaller. All rows still pass the compiled equality and visibility checks. A live Spine consumer table defers the whole rule to the existing router. An individual uncovered pivot can therefore remain on Free Join when another pivot in that rule has a possible Spine handoff.

Discovery enables every legal cut. A separate Cargo Bake command compares a completed discovery profile with the context-off profile of the same artifact. It checks Run schedules and per-rule logical binding counts. It groups rules by a typed incidence signature built from table aliasing, column positions, shared variables, literal values, table schema kinds and primitive semantics. Three rounds of coloring and sorted multisets erase table names, variable names and conjunction order. This is a cost heuristic. The complete matcher is checked separately for each target rule. A renamed and permuted chain in the test suite also exercises transfer across distinct sources.

For each family Bake retains the cut when ordinary candidate visits exceed discovery visits plus root reuse validations, port probes and reverse group probes. Wall time does not enter this decision. The policy is applied to the *target* recipe during Bake and carries its recipe digest. A selected target rule still faces runtime cardinality limits and Spine routing. The previous narrow matcher remains available for its eligible rules if a transferred policy has no cut for them. The same 64 iterative blocks and their RHS handoffs are active in the primary ablation. Break-even estimates or a learned block cannot certify the current contents of a partially bound relation.

## Results

Both frozen standalone Cell CPU programs contain 582 rules and yield 481 rules with cuts and 1,481 root orientations. Of the other 101 rules, 81 have fewer than two table atoms, 13 exceed the eight-table discovery budget, and seven have no complete cut under the connectivity and primitive scheduling conditions. The profiled runs cover 304 distinct eligible family signatures. Discovery actually supplied one or more Run answers (including complete empty answers) for 138 rules on sum-bits and 170 rules on Raytrace.

| Workload | Policy | Pass candidate visits | Inference median, ms | CLI median, ms | Paired inference Δ vs narrow, ms |
|---|---|---:|---:|---:|---:|
| sum-bits | Narrow control (#31) | 19,228 | 37.885 | 142.315 | — |
| sum-bits | All discovered cuts | 84,524 | 47.754 | 151.189 | +10.070 |
| sum-bits | Raytrace-trained | 19,227 | 37.546 | 140.937 | -0.365 |
| sum-bits | sum-bits-trained | 19,223 | 36.784 | 143.387 | -0.503 |
| raytrace | Narrow control (#31) | 8,417,882 | 13,711.227 | 16,477.401 | — |
| raytrace | All discovered cuts | 10,333,956 | 14,129.658 | 16,890.146 | +619.404 |
| raytrace | Raytrace-trained | 8,385,832 | 13,644.949 | 16,346.655 | +142.396 |
| raytrace | sum-bits-trained | 8,418,736 | 13,594.660 | 16,295.627 | +19.084 |

Raytrace training retained two families. One covers rule 249, the known loop invariant. The other covers rules 357, 359 and 361, which reject tuple operands of `Print`, `Free` and `Load`. For example, start from the `Load` operator, enumerate matching `Bop` nodes, read the type of each operand, then check it against `TupleT`. No offending operand was present in this workload. The cached prefix still has to track later arrivals and type changes. Sum-bits trained a different family (rule 457), a rewrite correlating tuple projections from the two branches of an `If`. Its training evidence consisted of a single admitted Run. Performance transfer is workload-dependent even with identical rules. Rule 22's arithmetic rewrite saved candidates individually on Raytrace but its family did not pass the aggregate admission criterion.

| Workload | Rule | Candidates, context off → discovery | Extra counted maintenance | Logical bindings | Kept by local training |
|---|---:|---:|---:|---:|---|
| sum-bits | 144 | 1,538 → 1,443 | 1,701 | 161 | no |
| sum-bits | 457 | 16 → 12 | 2 | 4 | yes |
| raytrace | 22 | 1,194 → 606 | 152 | 146 | no |
| raytrace | 249 | 381,729 → 152,208 | 10,282 | 143,282 | yes |
| raytrace | 357 | 0 → 5 | 32 | 0 | yes |
| raytrace | 359 | 5,657 → 2,484 | 1,267 | 0 | yes |
| raytrace | 361 | 50,691 → 21,819 | 10,934 | 0 | yes |

The survey counts residual rows and terminal/prefix pairs per rule. Maintenance in the admission model is an unweighted count of validations and probes. The full-pass candidate count also includes direct rows, merge candidates, Spine pivots and shared-node predicate checks. These are operation counts rather than unique facts.

Against the narrow control, Raytrace admission removed another 32,050 full-pass candidates (0.38%). Its unprofiled inference median fell from 13.711 to 13.645 s, but it won only one of four matched rounds and the median paired difference was +142 ms. The local type checks show why candidate savings alone are insufficient for throughput. In separate per-rule profiling, `Load` changed from 3.93 to 8.95 ms, retaining up to 1,269 prefix rows and issuing 10,911 port probes. The `Free` check changed from 1.11 to 1.43 ms. For both, ordinary Free Join also returns the empty result cheaply. All-cut discovery slowed down every measured Raytrace and sum-bits pair. Transferring the sum-bits policy to Raytrace added 854 candidates relative to the narrow control.

The expensive `Get`, `Single` and `Concat` rules maintaining `Expr-size` all received compiler plans. On Raytrace their eligible dirty Runs encountered active Spine consumer tables 1,679, 1,676 and 1,676 times respectively. The context matcher supplied zero answers for these rules in the primary comparison. In a second Raytrace audit with Spine disabled in both arms, these same 1,679, 1,676 and 1,676 requests all failed the root-cardinality admission check before decoding a root. Candidate counts for these three rules were unchanged at 966,545, 306,775 and 1,058,686 respectively. Both no-Spine arms had identical per-Run RHS digests and final graphs. A small context for this workload would need to start at an individual changed expression or unclaimed pivot, instead of retaining a whole root table.

The primary timing excludes offline profiling, Bake, loading, export and audit. All configurations use a single thread, the fixed 64-block iterative artifacts and the original source schedule. Times are fresh-process release medians with one excluded warm-up per lane, seven measured rounds on sum-bits and four on Raytrace. Order rotates across arms. The reported shortlist rule timings are separate profile-enabled medians over two measured processes after a warm-up. Each timing checks the schedule and final graph digests, operation counts, number of resumed Spine steps and maximum depth. Separate audited executions check every ordered RHS digest and the entire winning Spine prefix tree against the frozen references. [Per-rule discovery counters and raw timing samples](context-cut-results.json)

## Cargo reproduction

Use the `fixed.json` artifacts and environment from the [RHS handoff ablation](RHS_COMPLETION.md#reproduction). Use fresh output paths. Set `train` and `target` to the respective artifact files. Release builds for these artifacts require the iterative feature.

```sh
m=engine/cell-runtime/Cargo.toml
cargo run --release --locked --manifest-path "$m" --features iterative-bake -- \
  --program "$train" --inspect-program
for mode in 0 general; do
  CELL_CPU_PROFILE=1 CELL_CPU_LOCAL_CONTEXT=$mode cargo run --release --locked \
    --manifest-path "$m" --features iterative-bake -- \
    --program "$train" --output "discovery-$mode"
done
cargo run --release --locked --manifest-path "$m" --features iterative-bake \
  --example bake_contexts -- "$train" discovery-0/report.json discovery-general/report.json \
  "$target" admitted.json
CELL_CPU_LOCAL_CONTEXT=1 cargo run --release --locked --manifest-path "$m" \
  --features iterative-bake -- --program "$target" --output narrow
CELL_CPU_LOCAL_CONTEXT=general cargo run --release --locked --manifest-path "$m" \
  --features iterative-bake -- --program admitted.json --output learned
```

For a new small source, compile with `cargo cell-compile --source input.egg --emit-free-join-entry --output compiled` and run `cargo cell-bench --program compiled/program.json --lane free-join-planned --modules context-cuts --verify-stages`. The ordinary default remains off.

The follow-up starts at a changed expression and keeps the existing Spine coverage for each individual rule entry. [Delta-entry neighbor context ablation](NEIGHBOR_CONTEXT.md)
