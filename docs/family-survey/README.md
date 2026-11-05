# How much Pass 3 work can another shared consumer block remove?

Both frozen programs executed all 582 rules across 31 scheduled rulesets. Sum-bits produced bindings in 214 rules across 1,428 Runs. Raytrace produced bindings in 298 rules across 16,883 Runs. The main ranking starts with the standalone single-thread CPU at `9e4f9203` with `view-ports` enabled, so the benefit of the existing `CellSpineView9` block is already in the baseline. Measurements use the original schedule and 64-unit Bake artifacts.

**The new whole-family replays identify tens of milliseconds of potential savings on Raytrace.** On Raytrace, the sum of positive replacement estimates for the 29 replayed families, excluding the existing block, was 38.7 ms. This optimistic figure assumes the replacement can also preserve the required provenance. Restricting credit to windows with no observed live Spine routing or local binding metadata leaves no new complete family above 1 ms after charging its directories. Some of the largest repeated partial accesses are worth measuring separately.

## Priority on Raytrace

| Candidate | Measured scope | Estimated saving | What must still hold |
|---|---|---:|---|
| `SubTuple`, rules 137/138 | 39.2 ms original co-active search, 3.6 ms fused replay, 10.6 ms update cost. Scanning once versus separately with identical replay code saved 0.56 ms. | 24.9 ms for the complete replacement | 31,351 applied Spine resumptions across the two rules. Only 1/199 windows was unclaimed. Five rules read the table, so the model keeps its existing column index. |
| `BodyContainsExpr → DoWhile`, projection 21 | 479,631 repeated same-witness projection reads, of which 477,424 were already cache hits. | 20.5 ms repeat cost, about 19.0 ms after a snapshot-local shadow memo | Reuse only checked residual entry work. Exact downstream guards and ownership still run. The previous persistent semijoin experiment shows that a reduction in probes alone can lose overall. |
| `Expr-size` entry node | 4,712,198 cross-rule revisits in `always-run`. Complete admission for the co-active node costs about 841 ms, including different rule-specific joins, guards and routing. | 13.9 ms for replacing repeated full-tree frontier scans with an array | The array replay times enumeration only. It does not remove 841 ms. Normal entry projection, late inputs and capture still apply. |
| `TCons / TypeList-ith`, rules 576/577 | 15.8 ms original co-active search, 3.7 ms fused replay and 3.9 ms update cost. 10,869 late-peer output tuples before residual guards. | 8.3 ms for the complete replacement | 817 Spine resumptions and 0/351 unclaimed windows. The index has other readers. |
| Error checks 362/363 | Same admitted `HasType` entry and compatible receiver wiring. The warmed packet stream and witness domains compared equal. | About 1.3 ms for this entry | The whole multiway replay instead loses about 42 ms when charged for its required directories. Select the common entry only. |
| `Smaller` index, sole rule reader 287 | Physically replaced in a separate execution ablation, with 213,706 ordinary column updates removed. | No reproducible whole-inference gain | The replacement still publishes and refreshes the constructor-port directory. Measurements below. |

These estimates answer different implementation questions. A whole-family replacement includes the change in matching algorithm and output sealing. The split/fused replay comparison isolates the benefit of enumerating the same core once. Partial projection, whole-entry and raw-scan probes can cover the same CPU instructions, so their scores must not be added. The full table includes negative estimates.

On sum-bits all positive new whole-family replacement scores together amount to about 0.45 ms before provenance constraints. The top `SubTuple` estimate is 0.24 ms and the shared `BodyContainsExpr → DoWhile` memo is about 0.04 ms. Sum-bits remains useful for correctness and fixed-cost regressions.

## Budget for another 2×

| Scope | sum-bits | Raytrace |
|---|---:|---:|
| Clean complete inference median | 31.93 ms | 12,872.14 ms |
| Reduction needed to halve inference | 15.96 ms | 6,436.07 ms |
| Entire original search time of co-active rules reading the same multiset of tables | 1.60 ms | 315.70 ms |
| Entire batched admission time for other shared entry nodes, excluding those whole-rule calls | 3.13 ms | 1,588.76 ms |
| All existing column-index publication batches, even for unrelated readers | 1.36 ms | 995.92 ms |
| Generous work budget if all three scopes disappeared at zero replacement cost | **6.09 ms** | **2,900.38 ms** |

The work budget sums medians from separate instrumented passes. It explicitly removes the overlap between whole-rule calls and the batched admission scope. It gives away the entire column batch including any co-tenants. It is a sizing calculation with the original schedule and all other work fixed. Common intermediate joins in different rules, work outside the measured entry and read operators, a different fixpoint schedule, and fused RHS/publication algorithms can have different budgets.

On Raytrace 15 writers of `Expr-size` consumed about **2.56 s in search and 1.01 s in RHS**, producing 1,940,065 bindings, with 329,237 applied resumptions. Those numbers are the cost of the rules, not the removable duplicate. For example, rule 148 (`Function / Expr-size`) used 437 ms of search for 1,458 bindings and no applied resumptions. Rule 155 (`Single / Expr-size`) used 620 ms and all 306,775 bindings resumed a Spine. They should have different next experiments. For 148, split entry-domain selection and misses from actual matches. For 155, measure how much of producing and committing an already-supplied binding can be discharged by a switched block while preserving every ordinal. A common-driver scan alone accounts for little of either rule.

## Calibration and the removable-index experiment

With the existing consumer block disabled, its three `CellSpineView9` consumers cost 342.4 ms in co-active search. The shadow alternative cost 50.7 ms. With the existing consumer block enabled, ordinary observed search was 62.7 ms. The predicted search reduction was about 292 ms versus 280 ms observed in these coarse clocks. The earlier independent instrumented ablation measured 346.2 → 60.4 ms for the same consumers. This calibrates the search estimate, not inference time. In the old configuration the model conditionally credits 171.4 ms of ordinary view-index batches and charges about 100 ms for its shadow indexes. In the current configuration it gives that view zero index credit and ranks building it again as a loss. [Previous executable ablation](../VIEW_CONSUMER_BLOCKS.md)

`Smaller` has a different remaining opportunity. The only rule reading it is already the compiled block's producer. The ordinary column-only batches cost **87.8 ms** on Raytrace. An opt-in trial retires these columns and the driver shared-node refresh when every LHS reader is that producer. It maintains one exact join-key directory with an epoch frontier, using the actual repaired or retired constructor witnesses. Late changes accumulate until the next scheduled refresh. Other readers, smaller address spaces and unsupported plans retain ordinary execution. The primary graph directory and the peer's index stay available.

| Trial | sum-bits | Raytrace |
|---|---:|---:|
| Change headers / headers on Runs with an effective union | 212 / 109 | 213,762 / 124,647 |
| Live port rows at the end | 103 | 73,956 |
| Port notification + refresh, separate instrumented process | 0.005 + 0.036 ms | 11.58 + 54.81 ms |
| Ordinary → trial inference medians | 31.255 → 31.954 ms | 12,428.246 → 12,431.157 ms |
| Paired difference, faster rounds | +0.749 ms, 1/8 | +46.882 ms, 2/4 |

The ordinary `Smaller` column index was absent in every trial run. The difference between 87.8 ms and the approximately 66.4 ms of measured replacement maintenance is not a measured inference speedup. The trial includes driver enumeration, port lookup and different cache behavior. It remains opt-in.

## What was measured

- **Coarse controls.** Clock each real rule search on its immutable Run snapshot. Charge only genuinely active seminaive rules to overlap, and record watermark, actual bindings, residual visits and possible incoming Spine or local metadata. Per-rule RHS timers cover contiguous original commit ordinals. Real resumption counts are taken from applied steps. Table clocks measure the post-repair column publication batch, its other subscribers and whether the Run had any effective union. “Union Run” is temporal overlap, not proof that union caused each update.
- **Whole-family replay.** Start from all rules in each schedule. For one/two-table rules, group by a common keyed column pair, preserving literals, repeated columns and each member's supported primitive tail. For three or more tables require a common connected spanning tree over normalized table occurrences. Equal table occurrences retain their original order. Each rule is assigned once per set, preferring wider agreement. Disconnected or incompatible wiring, incompatible guards and other programs stay in the broader table-multiset ceiling. In these artifacts 29 families replayed successfully across 309 sum-bits and 2,949 Raytrace windows. The replay checked 1,332 and 271,964 ordered RHS tuples respectively. Raytrace had two budget fallbacks, sum-bits had none, and there were zero mismatching windows. The independent `same-tables` ledger includes 62 and 70 co-active multiset signatures, including cases that only share names.
- **Cost model.** Compare independently clocked original search against read-only fused execution on the same epoch and watermark. Charge cold build, updates on all intervening publications, and failed replay work. Replacing the matcher can save work beyond sharing. Split replay runs the same implementation member by member and alternates which replay runs first. Report rejected, fresh/fresh and late-peer cases separately where applicable. Cap an input table at 250,000 witnesses and a replay at 250,000 intermediate candidate visits. A failure or cap contributes no saved search. Charge each family the full measured cost of its watched axes even when another surveyed family shares them. This can overcharge a dedicated design. A positive model estimate requires a real execution ablation before adoption.
- **Index and provenance conditions.** Conditional index credit requires all static LHS readers in the family, all their observed active windows covered and no measured range or neighbor subscribers. It uses only column-only batch time. Dedicated ports, ad hoc probes and unobserved external uses still require a correct implementation or fallback. The conservative second score removes credit for any window flagged by Spine or local binding metadata while still charging the directory. Logical equality of ordered RHS values in a shadow stream alone does not authorize substituting physical witnesses.
- **Partial work.** Distinct probes measure actual cross-consumer repeated point reads, column-domain requests, and Free Join projections against the same driver witness. Clock-floor-adjusted repeat time is a gross price. The projection memo also times storing, retrieving and clearing positive and negative memberships within a snapshot. Its outer routing, residual effects and total deployment cost remain outside the estimate. The complete admission replay is stricter. Node, watermark, incoming Sieve guards, ordered projections and ownership direction must agree, and the pivot must have no scheduled cursor. Every resulting witness and ordered subtrie is compared to ordinary admission. First-store, release and receiver copy costs are charged. A separate raw-frontier replay times only the ordered B-tree walk versus collecting/reusing a vector. A later rule may reuse its first owner's vector for more than one pivot, whereas the lower-level revisit ledger counts a change of rule between consecutive visits for the witness.
- **Validation.** Three fresh diagnostic processes per measurement and workload, plus a separate combined audit. The coarse, replay and operator clocks were collected separately from clean release inference. All measured processes were checked against the frozen state and schedule. Combined diagnostics and the actual producer-port trial matched the ordered per-Run RHS, effective function writes, applied resumption streams, winning Spine tree, final graph and schedule. Sum-bits had 3,034 resumptions with maximum depth 12. Raytrace had 663,403 with depth 29.

## Files and reproduction

[Full-family rankings](full-families.tsv) · [Loose same-table ceilings](same-tables.tsv) · [Partial work and shared driver groups](partial-work.tsv) · [All 1,164 per-workload rule records](rule-times.tsv) · [All 802 per-workload table records and readers](table-costs.tsv) · [Replicate budgets, audit digests and port trial timings](measurements.json).

Run from the repository root. Use a fresh directory for each command and the same frozen program for all arms.

```sh
cargo cell-compile --case sum-bits --output out/sum
cargo cell-compile --case raytrace --output out/ray
sh docs/family-survey/reproduce.sh out/sum/program.json out/survey-sum
sh docs/family-survey/reproduce.sh out/ray/program.json out/survey-ray

cargo run --release --locked -p eggfuse-cell-runtime --features view-ports -- \
  --program out/ray/program.json --output out/ports-control
cargo run --release --locked -p eggfuse-cell-runtime --features family-port-trial -- \
  --program out/ray/program.json --output out/producer-port
```

For pre-block calibration use `family-survey` and `family-replay` without `view-ports`, then pass `-` for optional overlap, admission and raw-scan inputs to the Cargo ranker. Set `CELL_CPU_PROFILE=1` in a separate producer-port run for its notification/refresh timers. The default strategy and timing benchmarks do not enable the diagnostic features.
