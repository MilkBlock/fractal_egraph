# Shared conditions and event-published coordinates

**Maintenance fell from 0.980 s to 0.276 s, but the final provider still did not
beat the provider-off GPU control on the same raytrace Pass 3.** It remains
opt-in. The original seven rules, model weights, coordinate vocabulary,
schedule, native RHS and exact CPU complement are preserved in the main test.

## What changed

* Alpha-normalized residual projections share one index, retaining types,
  literals, aliases and the bound argument position. Nineteen per-consumer
  indices become six shared indices in the original scope.
* A condition block identifies an actual `(projection, binding key, version)`
  request. Consumers use compact block indices during a Run. A complete
  immutable witness vector, including an empty vector, is fetched once and
  reused. Local slot maps remain separate; independent column marginals are
  never substituted for correlated rows.
* Committed changes invalidate the affected block versions. Timestamp-only
  changes refresh semi-naive payloads without changing structural coordinates.
  This fast path recorded zero hits in this workload, so it receives no credit.
* Constructor events maintain compact typed node/row slots. Local changes have
  a direct publisher; cross-node dependencies subscribe to specific heads and
  row families, including absent ones. Union does not globally clear coordinates.
* Events coalesce dirty coordinates. At a consumer boundary, only coordinates
  needed by unknown condition blocks are published. Already exact blocks bypass
  prediction. GPU scores for the remaining blocks still only choose check order.

CPU completion remains exhaustive. The original native timestamp frontier is
checked before returning each complete binding. This is an execution-view
optimization; it does not introduce a new RipSat closure certificate.

## Final seven-rule results

Every changed lane below passed strict final original-table comparison.
Inference includes provider maintenance, matching, native effects and rebuild.
Construction, exports and extraction are separately measured. One run per
configuration, without selecting a best repeat.

| Configuration | Inference | Maintenance | Query / publication |
|---|---:|---:|---:|
| GPU, provider off — refreshed control | **17.714 s** | — | — |
| Original provider — historical | 20.613 s | 0.980 s | 0.256 s |
| Shared blocks, legacy coordinates — same final binary | 19.223 s | 0.939 s | 0.172 s |
| Event coordinates, sharing disabled — same final binary | 20.910 s | 0.336 s | 0.283 s |
| **Shared blocks + event coordinates** | **18.733 s** | **0.276 s** | **0.187 s** |
| Shared exact CPU blocks, no prediction/coordinates | 18.144 s | 0.082 s | 0.044 s |

Maintenance is **71.8% lower than the original provider**, and **70.6% lower
than the same-binary legacy-coordinate ablation**. The final lane is 2.55%
faster than that legacy-coordinate ablation. Its ordinary GPU device time is
almost unchanged (5.837 vs 5.832 s), supporting the local maintenance attribution.

However, it is **5.76% slower than the refreshed provider-off control**. Do not
use the older 18.828 s provider-off run to claim a small net win. The original
and intermediate runs are retained in `shared-results.json`.

The sharing-off ablation changes both cached answer reuse and the amount of
prediction requested. Its ordinary GPU device time also differs (6.260 vs
5.837 s), so its entire wall-clock difference is not pure cache credit.

**Phase accounting:** `maintenance_seconds` includes bootstrap ingestion;
`query_seconds` includes the first initialization call, including that ingestion.
They overlap once. Their sum is an upper bound, not an exclusive phase
decomposition. Net speed is judged from the full inference interval above.

## Real work, not cache hit rate alone

The final lane retains the same **25,247 affected anchors** and returns the same
**3,945 complete bindings**. Its 30,607 logical condition checks comprise:

| Work | Count |
|---|---:|
| Exact index materializations | **14,086** |
| Reuses of complete block answers | 16,521 |
| Of those, reuses of an empty answer | 10,485 |
| Version-cache probes when forming blocks | **34,337** |
| Witness rows materialized | 3,269 |
| Block invalidations | 2,315 |

The original guided lane performed 33,509 exact index reads; sharing-off still
does that work. We do **not** call 14,086 the total number of CPU lookups:
34,337 cache probes, block interning, delta routing and binding assembly also
cost time and are included in inference. No row-by-plan capacity is reserved.

Coordinate subscriptions are demand-created: 16,582 subscribed slots, 21,192
publications, and 21,770 requests. There are 388 new model/signature evaluations
in 40 GPU requests. Traffic is 39,032 bytes up and 2,032 down. The predictor
itself takes approximately 5 ms of device time. Generic matching still has
7,259 dispatches; removing these seven small queries does not remove those
dispatches.

The cached answers change the proposal population: old `predicted_*` field
names now describe combined exact-block/model routing. They must not be read
as standalone GPU precision or recall. `run.py` records this interpretation and
exports per-rule depth histograms and equal-rule-weight reach curves.

## Verification and unsuccessful directions

* **12 unit/regression tests passed**, including negative-cache invalidation,
  complete multi-witness payloads, late facts, aliases, union, subsume and
  function replacement. Coordinate tests include 150 successive structural
  updates and comparison with the uncached feature definition.
* The final seven-rule run passed **12,586 native full-binding audits** and
  **21,770 coordinate-reference checks**.
* Final **699,068 original table rows**, values, equivalence and visibility agree.
  Production Tiger selected identical DAGs for **29 functions**.
* The first eager publisher was counterproductive: 97,488 publications for
  34,337 requests, including dormant consumers. Maintenance rose to 1.496 s.
  Demand-boundary publication replaced it; the result is retained, not hidden.
* A separate automatic export admitted constructor/function residuals and
  analysis-relation anchors under the same checked single-bound-key contract.
  It selected **12 rules**, including three `BodyContainsExpr` / loop-context
  queries. The earlier expanded timing was 19.449 s, still slower than control;
  it does not replace the seven-rule comparison. Its final-code audit passed
  **21,576 native binding checks**, **83,037 coordinate checks**, and strict
  final graph comparison. The pre-compact expanded run also passed Tiger; no
  new extraction-quality claim is made for the audit-only final rerun.

The remaining issue is not the 5 ms matrix arithmetic. The provider still pays
for an additional context/index/routing layer to replace relatively cheap
existing GPU work. Wider coverage also grows anchor routing and binding
assembly work. The checked execution path is useful infrastructure, but these
results do not justify enabling it by default or claiming Cell-proof credit.

## Reproduction

Use the existing `build.py` and `run.py` commands in README. Final default:
`out/structural-shared-final`; same-binary ablations:
`out/structural-shared-legacy` (`--coordinates legacy`) and
`out/structural-no-shared-final` (`--no-shared-conditions`). The refreshed control
is `out/structural-shared-demand/baseline`; the unchanged same-commit CPU graph
control remains `out/structural-runtime-pass3-v4/native`.

Optional broader export: `export.py --residuals all --output FILE`, then
`run.py --model FILE`. This remains opt-in and rejects unsupported guards,
container columns, primitive bound keys and non-anchor-complete queries.
`shared-results.json` records all input/build/model identities, per-rule work,
stage counters, controls and development variants.
