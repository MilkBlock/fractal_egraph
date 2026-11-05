# Contexts for uncovered delta entries

A changed constructor supplies its child slots directly. A changed child attribute supplies its expression and value, and the existing Free Join domain identifies candidate parent constructors. A parent with additional children needs their current attributes to finish the match. These are the points at which a small demand-driven context can reuse work. The compiler emits a plan when a seminaive rule has a single constructor, reads a unary attribute on constructor children and writes that attribute on the constructor result. Pure scalar operations may combine the inputs. The compiler reuses its checked single-write recipe and register kernels. Each eligible pivot must have a complete kernel with any partial table expansion confined to that constructor. The artifact records the constructor, attribute, child-slot wiring and eligible pivots. Earlier artifacts derive the plan from the same typed recipe.

Spine first completes its existing scheduled entries. The residual loop tests coverage of the actual atom and fact reference before entering the neighbor plan. Existing Sieve, planned Free Join, range and storage admission still apply. The ordinary seminaive owner test, exact column comparisons, input ordering and RHS schedule also apply to locally supplied rows. A block switch supplied by the fixed iterative catalog already discharges covered work. Only the residual pivots offer evidence for this experiment.

The ablation has three modes.

- **Off** runs the previously planned residual kernels.
- **Ports** materializes attribute rows, including absent keys, on the first demanded sibling read. The cache is shared by consumers of the same attribute. The query indexer already decodes coalesced committed changes after repair. It forwards each subscribed attribute row and retirement to this cache. Replacements, key movement, late insertion and deletion update previously demanded entries. A table whose changes the indexer could not observe clears its stale cache. The cache has a cap of 65,536 demanded keys per attribute.
- **Ports + parents** also retains neighboring constructor rows for the actual Free Join or column-index domain reached by the changed expression. The first traversal both matches and populates the group. Later seeds can borrow the group. Groups sort by row epoch and witness, allowing a prefix to satisfy the seminaive ownership bound. A current source-table version and the original reference vector identify each cached group. Exact checks still handle fingerprint collisions. The bounds per constructor table are 4,096 groups and 65,536 source references. Singleton domains use the original traversal.

## Frozen Pass 3 results

Both targets use their fixed 64-block iterative catalogs with guarded RHS handoffs and the same Raytrace-trained context-cut policy from [the preceding experiment](CONTEXT_CUTS.md). Each ablation changes only the neighbor mode. These are single-threaded standalone Cell CPU measurements.

| Workload | Mode | Full-pass visits | Inference median, ms | CLI median, ms | Paired inference Δ vs off, ms |
|---|---|---:|---:|---:|---:|
| sum-bits | Off | 19,227 | 35.304 | 137.140 | — |
| sum-bits | Ports | 19,154 | 34.872 | 136.203 | -0.361 |
| sum-bits | Ports + parents | 19,154 | 34.957 | 137.122 | -0.250 |
| raytrace | Off | 8,385,832 | 14,252.025 | 17,066.387 | — |
| raytrace | Ports | 7,263,030 | 14,644.409 | 17,576.724 | +67.954 |
| raytrace | Ports + parents | 7,263,030 | 13,991.320 | 16,922.071 | -305.323 |

| Workload | Expr-size input | Spine-covered / residual entries | Rule candidate visits, off → full | Profiled rule search excluding maintenance, off → full, ms | Full demand maintenance, ms |
|---|---|---:|---:|---:|---:|
| sum-bits | Get | 293 / 71 | 117 → 117 | 0.156 → 0.162 | 0.004 |
| sum-bits | Single | 292 / 0 | 0 → 0 | 0.116 → 0.120 | 0.000 |
| sum-bits | Concat | 95 / 70 | 182 → 138 | 0.138 → 0.137 | 0.010 |
| raytrace | Get | 26,391 / 462,230 | 956,267 → 956,267 | 647.760 → 586.405 | 4.026 |
| raytrace | Single | 1,132,388 / 0 | 0 → 0 | 622.309 → 636.739 | 0.000 |
| raytrace | Concat | 11,768 / 422,631 | 1,052,080 → 533,296 | 465.325 → 442.887 | 2.843 |

| Workload | Mode | Demand maintenance, ms | Shared publication, ms | Profiled full-pass search excluding demand maintenance, ms |
|---|---|---:|---:|---:|
| sum-bits | Off | 0.000 | 0.000 | 16.886 |
| sum-bits | Ports | 0.010 | 0.034 | 16.549 |
| sum-bits | Ports + parents | 0.019 | 0.035 | 16.861 |
| raytrace | Off | 0.000 | 0.000 | 6,300.475 |
| raytrace | Ports | 2.726 | 89.341 | 6,177.624 |
| raytrace | Ports + parents | 8.803 | 92.656 | 6,241.479 |

The compiler found 14 rules in each 582-rule artifact. Raytrace exercised residual seeds of 11 of them. In the Get rule every one of the 956,267 residual neighbor visits emitted a binding. The parent context served 592,912 of them from already decoded rows. Spine covered every delivered Single entry. Concat examined 526,040 parents and requested one sibling attribute per parent. In the full context 518,784 of those values came from a demanded slot and 7,256 needed graph key probes. It produced the same 330,964 residual answers. Across rules the context saved 1,122,802 search candidate visits. Shared publication checked 1,126,308 already decoded attribute changes, refreshed 624,772 demanded slots and retired 1,138. This work is included in inference time.

The rule candidate count charges each physical point probe and each neighbor examined, including a cached parent traversed to emit an answer. Building a group uses the same first traversal. Its build-visit counter is a subset of those visits. Direct reads of an already bound attribute slot are counted separately. The full-pass number also includes direct rows, Spine pivots, merge candidates and shared-node predicate checks. Publication checks and refreshes consume CPU time and have their own counters.

Profiled rule search includes Spine dispatch, Free Join, matching and binding finalization. In the cost table, search subtracts on-demand context maintenance within that rule. Shared publication occurs after repair, outside rule matching. It is charged once for the whole program. The profile instruments context fills, evictions, group copies and sorting, plus maintenance of subscribed attributes in the publication loop. Ordinary cache probes stay in search time. Shared index traversal and notification dispatch are also present in the end-to-end inference timer. Instrumented component times are diagnostic estimates. Inference and CLI medians use separate processes with the profile disabled.

One excluded warm-up per lane precedes seven measured rounds on sum-bits and four on Raytrace, rotating and reversing the order. Profiles use three measured rounds after warm-up. With the full context, sum-bits was faster in 4 of 7 matched rounds with a median paired difference of -0.250 ms, and raytrace was faster in 4 of 4 matched rounds with a median paired difference of -305.323 ms.

With instrumentation disabled, Raytrace's median full-pass search phase (including demand maintenance) was 6.201 s off, 6.314 s with ports and 5.963 s with ports plus parents. The corresponding query-index update phase, which includes shared publication, measured 2.723, 2.842 and 2.746 s. Phase medians need not sum to the median of the full process. The full context reduced the Raytrace inference median by 1.83%. Sum-bits varied in direction. The new path is opt-in.

Every timed trial verifies the final graph, schedule, logical binding counts, facts created, function updates, unions, number of Spine resumptions and maximum chain. Separate audit builds compare the ordered RHS input digest for every Run byte-for-byte, and compare every entry of the winning Spine prefix tree to frozen references. [Paired timings, query costs and coverage counters](neighbor-context-results.json)

## Reproduction

Use each workload's admitted target artifact from the Raytrace-trained arm in [the context-cut study](CONTEXT_CUTS.md#cargo-reproduction). Set `program` to that file and choose a fresh output directory for each command.

```sh
m=engine/cell-runtime/Cargo.toml
export CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 CELL_CPU_FREE_JOIN=planned
export CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1
export CELL_CPU_KNOWN_RESULTS=static CELL_CPU_RANGE_GUARDS=1 CELL_CPU_SLOT_TRANSFER=off
export CELL_CPU_FD_KEYS=0 CELL_CPU_SCALAR_MERGE=0 CELL_CPU_LOCAL_CONTEXT=general
for mode in 0 ports 1; do
  CELL_CPU_NEIGHBOR_CONTEXT=$mode cargo run --release --locked --manifest-path "$m" \
    --features iterative-bake -- --program "$program" --output "timed-$mode"
  CELL_CPU_PROFILE=1 CELL_CPU_NEIGHBOR_CONTEXT=$mode cargo run --release --locked \
    --manifest-path "$m" --features iterative-bake -- --program "$program" --output "profile-$mode"
  CELL_CPU_NEIGHBOR_CONTEXT=$mode cargo run --release --locked --manifest-path "$m" \
    --features iterative-bake,spine-shapes,slot-transfer-audit -- \
    --program "$program" --output "checked-$mode"
done
cmp checked-0/rhs.json checked-ports/rhs.json && cmp checked-0/rhs.json checked-1/rhs.json
```

For a new source, use `cargo cell-compile --source input.egg --emit-free-join-entry --output compiled`. The small-source ablation is available through `cargo cell-bench --program compiled/program.json --lane free-join-planned --modules neighbor-context --repeat 5 --verify-stages`. Use `neighbor-ports` for the attribute-only arm.
