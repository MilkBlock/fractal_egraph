# Continuous raytrace runtime result

Actual production-derived Pass-3 packet, original 14-rule schedule. One fresh release execution per lane, Apple M3 Max, one native thread; no averages. Current native fork, not pristine. Offline prefix replay and clones are excluded from both lanes. No solver calls or per-round shader compilation.

| Full packet | Seconds |
|---|---:|
| Native run | 8.164158 |
| Native search + apply | 6.969651 |
| Native insert/merge | 0.058193 |
| Native rebuild | 1.081601 |
| Assisted one-time setup | 0.567088 |
| Assisted continuous run | 11.276161 |
| Assisted setup + run | 11.843249 |

## Assisted run breakdown

| Component | Seconds |
|---|---:|
| CPU event routing | 0.409650 |
| CPU fragment projection/encoding | 2.693858 |
| Binary GPU round trips | 5.596474 |
| GPU device work (subset of round trips) | 1.235048 |
| Binding decode/sort | 0.090260 |
| Acknowledgements | 0.893214 |
| Original native RHS + merge/rebuild | 1.550540 |

Device time is not additive to roundtrip time. Native search+apply includes RHS staging. The assisted native action timer contains its private binding lookup and original RHS, not the original full relational Join. All host work remains in the assisted run total.

## Evidence that this is incremental

| Measure | Value |
|---|---:|
| Rules / worker installations | 14 / 14 |
| Native / assisted ruleset runs | 1,017 / 985 |
| GPU delta dispatches | 2,673 |
| Quiet rule calls, no GPU query | 927 |
| Old bootstrap fragment rows indexed without matching | 274,601 |
| First uploads, bytes | 11,988,136 |
| Later uploads, bytes | 93,556,936 |
| Downloaded binding records, bytes | 9,054,512 |
| Native logical matches / assisted emitted bindings | 285,337 / 287,499 |

Old rows are retained across native events and compiled plans survive the entire packet. Only new accepted bindings are returned in the timed lane. Match counts are not committed mutation counts. Rule-level workers currently duplicate shared tables; this is not one globally shared GPU table store.

## Correctness gates

| Gate | Result |
|---|---|
| Strict final rows/values/equality/visibility | False |
| Exact graph after explicitly excluding Smaller rows | False |
| Same ExtractedExpr cost sets | True (6,985 keys) |
| Native handback produces no further graph change | True |
| Separate native per-boundary delta-query checks | 3,600 passed |

Full graph difference: different final table counts: [("Smaller", Some(72049), Some(73234))]

Projection difference: missing final fact TermSingle [Eq("Term", Value(491516))]

Equal Expr/Term counts are not a graph-equivalence proof. Bootstrap fragment counts include repeated projections of the same native table across rule workers.

The Smaller-excluded check still verifies every other table, primitive value, visibility and equality modulo IDs; it is not a node-count approximation. It is reported as a separate projection, never a replacement for strict whole-graph equality. Equal costs do not by themselves prove identical selected syntax. No actual compiler extraction or frontend rerun occurs inside this isolated packet.

| Sort | Native nodes / classes | Assisted nodes / classes |
|---|---:|---:|
| Expr | 69,455 / 23,155 | 69,455 / 23,155 |
| Term | 62,749 / 62,749 | 62,749 / 62,749 |
| TermAndCost | 134,798 / 62,749 | 135,983 / 62,749 |

Total table rows: 655,832 / 657,017.

**No validated strict same-graph speedup is claimed.** The continuous run and the cold-query experiment are distinct. This runtime currently uses explicit table fragments; it does not exploit baked Cell guarantees to omit internal LHS atoms. Certified Cell coverage is unmeasured, not assumed complete.

The separate checked replay is excluded from all timings above. Its executable/source/worker provenance is checked; the timed and checked reports retain all result gates separately. Sum of per-worker allocated buffer peaks is recorded in JSON; it is not process RSS.

Raw outputs: `/Users/mineralsteins/.codex/worktrees/cell-assisted-egglog/egg_layout/out/raytrace-live-version-bench` and `/Users/mineralsteins/.codex/worktrees/cell-assisted-egglog/egg_layout/out/raytrace-live-version-checked`. See README for reproduction and the native handoff contract.
