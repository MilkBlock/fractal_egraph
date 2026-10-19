# Raytrace Pass 3: timing obtained, strict result gate fails

One final release run per lane on Apple M3 Max, one native thread, probe counters
and query audits off. Full captured Pass-3 main schedule, with only the 14 terms
family rules assisted. This is the modified native runtime, not pristine egglog.
See [README.md](README.md) for scope and reproduction.

| Lane | Main schedule (s) | Extra runtime setup (s) |
|---|---:|---:|
| Native original fused execution | 22.640870 | — |
| CPU cached queries + same sorted commit | 22.973929 | 0.002751 |
| Hybrid GPU + same sorted commit | 17.522760 | 0.060030 |
| Certified whole-Cell sat skip | **unavailable** | — |

**These are execution times, not an accepted equivalent-result speedup.** The
GPU run finishes and passes the original program's executing checks, but differs
from both the fused baseline and the same-commit CPU control under the stronger
final graph comparison. No speedup ratio is certified. Sat skip has no admissible
library and skips **zero** boundaries, so none of this timing is a Cell benefit.

Shared source/schema/input initialization: 0.248250 s, outside every main timer.
Clone times are in the JSON; full verification and candidate-cost inspection are
also outside the timers. No new Tiger extraction or peak-RSS measurement was
performed. The older development runs are not averaged into this final run.

## Work and time breakdown

The original native lane executes 3,093,624 logical matches in 10,120 native
iterations: search/apply 18.076640 s, insert/merge 0.316345 s, rebuild 3.719407 s.
This is genuinely search dominated. Matches are not committed mutation counts.

| Hybrid GPU component | Seconds |
|---|---:|
| Other rulesets, still native (6,430 runs) | 12.405738 |
| Selected rules' native RHS/commit/rebuild | 2.058587 |
| GPU request/response, including device work | 2.682002 |
| Device work, **inside** the preceding row | 1.062413 |
| Commit-event handling | 0.244569 |

These counters are not an exhaustive disjoint sum. The selected cached CPU
queries take 8.925987 s. CPU and GPU emit the same per-rule binding counts,
337,547 total. GPU makes 3,200 dispatches, uploads 40,152,352 bytes including the
2,218,384-byte initial upload, and downloads 20,252,820 bytes. Neither query
count equality nor these bytes prove final semantic equivalence.

The hybrid schedule visits 4,263 nested saturation invocations. That is not the
same denominator as the prior production outer-boundary profiler. None receives
a whole-Cell certificate in this experiment.

## What agrees, what does not

| Check | Result |
|---|---|
| Initial Expr anchors | 12,701 |
| Anchors with resolved candidate costs in both engines | 4,922 |
| Unresolved in both | 7,779 |
| Different anchored TCPair cost sets | **0** |
| Final active Expr e-nodes / e-classes | 70,884 / 23,659 in both |
| Full graph vs same-commit CPU | **FAIL: different equality partition** |
| Separate CPU control vs CPU control | **PASS** |
| Separate GPU native-frontier audit | **13,250 exact checks passed** |

The final graph checker anchors pre-existing IDs and maps new IDs by facts, so
it does not require allocations to use the same numbers. Its same-commit failure
is `different equality partition at Eq("Expr", Value(1972)) -> Eq("Expr", Value(1991))`.
Equal Expr counts do not imply equal membership in those classes.

| Final table | Original fused native | Hybrid GPU |
|---|---:|---:|
| Smaller | 72,198 | 73,372 |
| TCPair | 63,051 | 63,038 |
| TermConcat | 3,620 | 3,607 |
| All table rows | 697,922 | 699,070 |

The two CPU-control diagnostic runs agree on 699,069 rows and the full anchored
equality/visibility mapping. Thus native-vs-native stability does **not** explain
away the GPU-vs-control discrepancy as generic randomness.

The audit compares GPU query results against native queries on the GPU lane's
own current graph, not against the independent CPU lane's historical graph. It
establishes local frontier agreement for that diagnostic execution; it does not
certify the entire unaudited run. Its query execution also changes native caches
and timestamp bookkeeping. With audit enabled, some native-only rule counts
match CPU where the unaudited run differs. The workload contains order-sensitive
`:merge new` (`to-hoist`); that is a relevant investigation target, **not a proven
root cause**. First-divergence tracing across schedule boundaries remains needed.

## Rejected diagnostic and measurement correction

A clock-only native query barrier was tested because the CPU control's cached
queries advance a timestamp while unaudited GPU queries do not. It did **not**
remove the final graph difference. The speculative engine change was reverted;
the final timing above contains no such barrier. Its failed result is retained
as a small diagnostic summary in `results/diagnostics.json`.

An earlier cost metric compared final numeric Expr IDs directly. That is valid
only when every compared ID belongs to the common initial checkpoint; it is
wrong for a full pass that allocates new Exprs. We replaced it with input-anchored
comparison and regression tests. The earlier claim of 2,329 differing cost keys
is withdrawn. **Zero anchored cost differences still does not waive the full
graph failure**, and is not a completed end-to-end extraction quality test.

## Consequence for sat skip

The workload is suitable and expensive enough. The current Cell proof contract
is not yet suitable: function updates, custom merge consequences and the combined
terms saturation must be certified before skipping their internal matches. The
first admission error is a foreign function declaration (`LoopNumItersGuess`),
but the selected subsystem itself has `ExtractedExpr :merge (Smaller old new)`
and four numeric guards. A syntax-compatible GPU matcher is not that certificate.

Next correctness work is to isolate the first cross-backend graph divergence,
then extend the Cell certificate to the actual terms environment. Reporting the
17.52 s as a successful sat-skip acceleration would be incorrect.
