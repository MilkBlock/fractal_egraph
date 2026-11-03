# Pre-search context reuse: metrics and controlled experiment

2026-11-03. Single-thread standalone Cell CPU; authenticated sum-bits Pass 3
`source → source_sk → source_sk_mk → cell-cpu` artifact. All lanes retain
`CELL_CPU_MERGE_PROGRAMS=1`. No raytrace, native rerun, GPU, or new Bake budget.

**Result: complete positive contexts can be reused before residual matching,
including marker consumers, with identical bindings and stage states. This
workload does not show a useful net speedup. Leave the experiment OFF.**

## Measure before changing execution

`CELL_CPU_PRESEARCH_SURVEY=full|1` records actual work after a delta seed has bound
its fields but before remaining LHS work. `full` keys by the whole seed binding;
`1` projects out fields the remaining query does not read. Keys retain the rule,
pivot and correlated values. It records first encounters, changed dependencies,
and unchanged same-Run/later-Run encounters, along with actual candidate visits,
decoded rows, key reads and bindings. Dependency versions cover all remaining
query tables, so this is conservative, not a complete locality detector.

| Observation on the same 67,807 entries | Whole binding | Projected binding |
|---|---:|---:|
| Distinct contexts | 58,978 | 48,860 |
| Stable same-Run encounters | 0 | 4,427 |
| Stable later-Run encounters | 1,058 | 3,411 |
| Actual candidate visits in stable encounters | 113 | 2,623 |
| Actual key reads in stable encounters | 195 | 2,238 |

This is observed work, **not a count of safely skippable searches**. In particular,
the semi-naive pivot can change which part of an unchanged domain belongs to a
Run. Timing this observer is ineligible for performance claims. Direct one-table
rules, initial fact loading, and probes are outside this delta-seed survey.

## Executable experiment

`CELL_CPU_SEARCH_CONTEXTS` defaults to `0`:

- `1`: positive-context reuse for residual joins of at least two tables, plus
  the existing checked merge/marker entry programs.
- `marker`: only the checked entry programs. The flag includes merge constructor
  entries and their marker relays; it is not an assertion that every entry is a
  source marker or a certified RipSat Cell.
- `observe`: perform the original query even on a hit, compare the proposed
  bindings and epochs, and measure actual query work that replay would replace.

A complete residual query is alpha-normalized, including constants, bound values,
repeated-variable constraints, table/primitive types and atom order. Its answer
context stores the remaining correlated values and each contributing witness
with its epoch. Unrelated root/tag values come from the new caller; they are not
copied from an old answer. Multiple candidates remain separate rows.

Admission requires an ordinary successful query with **zero delta exclusions**.
A delta-truncated search cannot establish the whole domain. Empty answers are
not cached. The implementation performs no extra full-domain search to fill a
cache and does not add a heuristic capacity/selection threshold.

On reuse, exact read-range leases must remain valid; every answer is checked
against the current latest-witness/atom partition before becoming a Binding.
The normal sealed StagePatch then executes all original RHS instructions. No
constructor, function update, guard effect or union is declared redundant here.

After publication, only contexts subscribed to modified tables are considered
for invalidation, and their exact buckets decide validity. Fact deletion,
subsume, function update, union repair and witness relocation invalidate affected
reads. Hash collisions enlarge read ranges; they never prove equality. Index
lease maintenance is enabled per table when a read stamp is requested, not
globally for every ordinary-query index. Cache state is discarded on checkpoint
restore and recomputed through ordinary matching. It does not pin Cell owners.

This is **online exact query-answer reuse** integrated into the Cell engine, not
a new static RipSat completeness certificate. Static terminal sharing is still
subject to the previously documented custom-merge/container restrictions.

## Final same-binary measurement

One clean invocation per candidate, with the same off lane reused for the second
scope after checking input, binary, machine and configuration fingerprints.
Build, observer and audits do not overlap these timings.

| Metric | Off | Checked entries only | Entries + general residual joins |
|---|---:|---:|---:|
| Inference | 137.946 ms | 137.673 ms | 168.745 ms |
| Relative time | 1 | 0.998 | 1.223 |
| Actual candidate-row visits | 87,218 | 86,920 | 85,574 |
| Key lookups | 62,482 | 62,482 | 61,907 |
| Positive context hits | 0 | 298 | 648 |
| Hits across Run epochs | 0 | 291 | 596 |
| Hits from a different rule | 0 | 0 | 3 |
| Marker-anchored comparison hits | 0 | 119 | 119 |
| Completed bindings replayed | 0 | 298 | 851 |
| Extra witness reads while recording contexts | 0 | 115 | 3,568 |
| Read leases actually checked | 0 | 2,149 | 36,619 |

Candidate visits sum `stats.residual_candidates` and
`multi_stage.stats.merge_candidate_rows`; those counters cover disjoint paths.
Replayed answer rows and extra witness reads are reported separately, not hidden
inside a gross search-saving number. All lanes preserve 12,930 logical bindings,
19,199 constructor requests, 3,208 union requests, 2,176 merge calls, 2,378 repair
visits and 4,518 final logical records. Logical bindings are not committed mutations.

The 0.2% entries-only difference is **not evidence of a stable speedup**. The
broad method saves 1,644 candidate visits (1.88%) and 575 key reads (0.92%), but
spends more on context keys, answer capture, dependency evidence and invalidation.

| Phase, broad method | Off | On |
|---|---:|---:|
| Search, including context acquisition/replay | 73.193 ms | 95.343 ms |
| Query-index update / context invalidation | 5.081 ms | 11.524 ms |
| RHS | 26.056 ms | 26.947 ms |
| Repair | 14.837 ms | 15.306 ms |

Of 19,495 context probes, 16,965 have no complete positive answer, 866 encounter
delta exclusions, 1,016 record a positive context and 648 hit one. The large
number of probes does not mean there was a large inner join to eliminate.

The shadow ledger separates the 1,644 saved candidate visits:

- 938 belong to auxiliary structural-key rules;
- 179 belong to the `CellSpineView10` publisher;
- 527 belong to original consumers, including 119 marker-driven comparisons.

The 298 compiled-entry hits consist of 179 publisher and 119 comparison hits;
each replaces just one `TCPair` row read. Existing source markers already expose
the other side's term/cost. These savings must not all receive RipSat closure credit.

Earlier implementation attempts are retained in `presearch-results/measurements.json`:
global cache validation was 135.821→196.455 ms; table-triggered validation of only
general residuals was 140.142→165.610 ms. Adding compiled entries with global index
lease maintenance was 132.251→164.076 ms (all) /145.709 ms (entries). These are
successive code variants with their own controls, not repeated samples or averages.
The final per-table lease version above does not prove the exact attribution of
every between-build millisecond; no profiler decomposition was performed.

## Correctness and limits

Both final candidates passed **1,511 exact snapshots**: all tables including
markers, full bindings, visibility, Run paths, repeat progress and change flags.
The endpoint and source schedule also match in clean timing runs.

Eight regression tests include the 52 existing semantic fixtures, shared inputs
across roots/rules/stages, late inputs, multiple union alternatives, deletes and
subsume, mutable function updates under forced fingerprint collisions, a cached
answer rejected by a different delta pivot, real merge-marker entries and cold
checkpoint resume. Dedicated new fixtures also compare with native egglog. Five
Rust library tests pass.

The complete marked Pass 3 still has the previously recorded native `TermBop`
wiring discrepancy; this experiment proves off/on equivalence, not a new full
native or extraction equivalence result. It does not establish a speedup over
original `source.egg` on native egglog.

## Interpretation and next step

The useful abstraction is the **remaining read projection**, not the whole
historical application binding. However, retaining general positive answers is
too expensive here and the existing marker path has already made individual
reads cheap. More cache entries alone will not address this measured imbalance.

The next useful experiment should publish a shared, multi-read execution context
directly from existing producer/marker events, preserving its input subscriptions
and correlated alternatives. Measure original-consumer candidate visits saved
per context construction/update and net search time. It must demonstrate work
saved before matching; repeated RHS hashcons hits or auxiliary-key savings alone
are insufficient. Do not enable the current broad cache by default.

## Reproduce

```
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml --bin eggfuse-cell-runtime --example project_stages
python3 -B -m experiments.cell_runtime.presearch_bench --compiled out/known-static-compile --output out/presearch-survey-new --survey
python3 -B -m experiments.cell_runtime.presearch_bench --compiled out/known-static-compile --output out/presearch-shadow-new --shadow
python3 -B -m experiments.cell_runtime.presearch_bench --compiled out/known-static-compile --output out/presearch-measure-new --audit
python3 -B -m experiments.cell_runtime.presearch_bench --compiled out/known-static-compile --output out/presearch-entry-new --method marker --reuse-control out/presearch-measure-new --audit
```

For this worktree, tests reuse the existing native frontend through
`EGGFUSE_TEST_FRONTEND=/Users/mineralsteins/.codex/worktrees/d569/egg_layout/target/debug/examples/eggfuse_frontend`.
Run `python3 -B -m unittest experiments.cell_runtime.presearch_test`.

The compiled upstream artifact and its pass receipts are reused, not rebuilt
from an unmarked raw `source_core`. New runtime recipes come from the validated
typed program; this does not upgrade the frozen artifact's old failed RipSat
certification receipts. Compact results retain source/program/binary fingerprints.
