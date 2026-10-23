# Runtime-priced layouts over the full eggcc passing corpus

The runtime-priced selector avoids the previous initial-table-size mistake.
On the saved full raytrace Pass-3 input, it is **1.81% faster in one paired run**
than the existing eight fixed parameterized query plans. It selects **no shared
marker**. This is a query-layout result, not new RipSat proof credit or evidence
of a large additional GPU speedup.

## Collected evidence

* All **96 passing workloads** (83 Bril, 13 Rust), their actual production
  extraction chain, and **288 pass inputs** completed.
* Target-engine replays recorded **411,633 Run boundaries**, including Runs
  inside Saturate. Every admitted alternative's complete bindings were checked
  against the native query at that frontier; only original rules executed RHS.
* There are 609 admitted query shapes, 514 observed shapes, 1,700 alternative
  plans (1,508 observed), and 4,881 structural prefix bodies. The 336 distinct
  rejected rule identities across the corpus remain native. Main rejection
  classes are unsupported guard/query forms, disconnected constructor-root
  plans and container columns. This is not coverage of every original query.
* Per-table updates/cardinality, per-stage reads/candidate visits and prefix
  cardinalities are measured. These are alternative CPU query-provider costs,
  not original Free Join or GPU stage costs. They are work units, not time fits.

The corpus is training evidence and includes raytrace; this is not a held-out
generalization experiment. The native target replay and the production engine
are distinct. Production extraction, not the diagnostic replay, supplies the
next pass's input.

Artifacts: `out/marker-workload-all-passing/manifest.json`, compressed per-pass
profiles, and `out/marker-workload-prices.json`. Their fingerprints and selected
plans are recorded in [results.json](results.json). Incomplete attempts were
excluded. One interrupted attempt and one abnormal Raytrace diagnostic exit are
preserved; completed passes were reused. The final attempt succeeded. Its peak
RSS was 27,054,325,760 bytes; trying all plans is expensive offline analysis,
not representative production memory use. This profile's elapsed time is not a
benchmark. Generated incremental compiler caches were removed when disk space
became low; measurements, executables and sources were retained.

## What the solver selected

The eight-query workload is unchanged. Root choices, prefix lengths, payloads
and shared/private choices remain in the original typed layout domain. All
selected plans are private with prefix length zero; 2,219 eligible choices
reduce to 739 by the exact incumbent bound. No heuristic candidate budget was
introduced. The reported objective is 12,750,628 work units, not nanoseconds.

| Consumer | Fixed root | Runtime-priced root |
|---|---|---|
| Get starts Add-Gets | Get | Get |
| Add-Gets through Concat | Add-Gets | Add-Gets |
| Add-Gets through Single | Add-Gets | Single |
| Add-Gets with Not-Just-Concat | Add-Gets | Not-Just-Concat |
| Add-All-Gets | Add-All-Gets | Add-All-Gets |
| Loop lsr-inv | DoWhile | DoWhile |
| non-weakly-linear If | If | Arg |
| state-edge-passthrough If | If | Arg |

Unlike the old static selector, it does not start these If searches at HasType
or the loop search at HasArgType merely because those tables are initially empty.
The whole-corpus objective is not an optimum for each individual raytrace rule:
state-edge-passthrough visits rise from 6,121 to 9,203 non-root rows, while the
other If query falls from 3,255 to 1,931. These local regressions are retained.

## Same-binary actual-runtime comparison

Both timing lanes use the same Rust executable, frozen Metal worker, source,
schedule, stable choice policy, existing joint proofs and eight-query family.
Each runs once without diagnostic counters. Previously verified CPU/native
controls are reused; new complete-graph and Tiger checks are performed. Setup,
profiling, solver work, graph export and extraction are outside inference time.

| Metric | Fixed parameterized plans | Runtime-priced plans |
|---|---:|---:|
| Inference seconds | 12.810308250 | 12.578382667 |
| Cell query service seconds | 0.768457386 | 0.708201035 |
| CPU root checks | 130,928 | 114,322 |
| CPU index reads | 100,257 | 80,925 |
| CPU non-root row visits | 184,907 | 186,665 |
| GPU candidate visits, separate diagnostics | 7,193,043 | 7,193,043 |
| Shared marker definitions | 0 | 0 |

All 305 common GPU query depth distributions are identical, including the
fixed-rule-weight survival curves. This change does not save another GPU join
layer. Most reduction in those CPU counters comes from the Not-Just-Concat
consumer: its root checks fall
from 25,972 to 6,220 and index reads from 22,371 to 5,495, with unchanged outputs.
The measured Cell service reduction explains only about 60 ms of the 232 ms
paired difference; do not attribute all timing variation to that counter.

Both graphs match the same-submit CPU in all original rows, values, equality
and visibility: **699,068 rows / 524,631 mapped e-classes**. Production Tiger
selects the same DAG for **29 functions**, with 8,006 selected rows. A separate
audit checks **41,361 Cell-supplied queries**. The older fused-native versus
same-submit discrepancy remains, and the stricter extraction command retains
its nonzero exit. This is not a pristine-egglog equivalence claim.

## Source-marker integration and what remains

`selected_source` now lowers parameterized shared prefixes through the existing
source publisher path. The If fixture shares ten structural atoms; the numeric
fixture publishes the successor marker using the actual `n+1` RHS value, without
re-reading that structure. Mutable reads/guards remain residuals and repair for
new inputs/union-enabled combinations remains. See
[the source tests](../parameterized_source_markers/README.md).

The full-corpus selection above contains no materialized prefix. Its source
output is therefore byte-identical to the input and retains all eight runtime
plans; it must not silently invent one-atom markers or drop the providers.
`out/runtime-priced-pass3/source` records this explicitly. This does not complete
the separate performance gate for a meaningful explicit SK+marker rewrite.

Private plan work is measured. Shared maintenance still coalesces same-version
census work using a maximum rather than the exact union of dirty root sets;
residual census is unfiltered. In addition, this runtime-cache model does not
measure how many source view-completion joins direct RHS publications would
remove. Consequently, selecting zero markers does **not** prove source markers
are useless. Before claiming their benefit, profile actual publications versus
repair, then feed that measured maintenance work into the same solver. The
new corpus fixes bad root pricing; it does not solve that remaining cost model.

## Reproduce without redoing completed work

Use the versioned `cell-assisted-egglog-bench` skill. Its
`references/planner-workload.md` documents `planner-corpus --resume`, explicit
failed-pass retry, `planner-prices --jobs 8`, `spine-program --runtime-workload`,
and source lowering. Static pricing now requires `--static-cost-experiment`.
The existing `boundary-credit --layouts` produces the comparison and depth
metrics; no new benchmark runner was introduced. Reuse the saved corpus for
future weights/layout experiments. The normal runtime compiles out the
`planner-profile` instrumentation.
