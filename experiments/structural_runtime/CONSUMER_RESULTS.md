# Consumer-scoped resident marker responses

This is an **actual saved Raytrace Pass-3 runtime experiment**, continuing the
ridge predictor together with existing joint-closure and loop-context services.
It is not a standalone matrix toy, and it does not add a new RipSat closure proof.
The original model, 68 admitted consumers, native rules and exact completion are
unchanged. Unknown/negative predictions still receive exact CPU completion.

The implementation reduces matrix work, but **does not establish a reliable net
speedup**. Keep it opt-in: `--resident-scope consumers`. The default `sort` path
does not build the new CPU subscription ledger. Ordinary nonresident matching is
unchanged. This does not reach the previously requested 6.17-second target.

## What is shared

Previously a changed context computed every nonconstant head of the same sort.
For Expr that can mean 16 conditions, even when the published anchors use only a
few. A context is a typed native value plus its current Analyze feature signature;
it is not a RipSatCell instance count.

We now retain the exact **consumer connections** already encountered while
publishing/removing native anchor rows:

```
anchor row -> (typed binding value, condition-model ID)
                     |
                     +-- ContextOf condition
                     +-- ExprIsPure condition
                     +-- tuple-length condition
                     ... only actual consumers, with support counts
```

Duplicate publishers increment support. Removing one publisher does not remove
another's connection. Native removal/replacement/visibility events update the
same counts as the existing anchor ports. Typed namespaces prevent IDs of
different sorts from sharing scopes. Direct demand is always admitted, even if
absent from a published scope; scopes never exclude a legal match.

A sparse binary scope record accompanies requested resident contexts. It is
independent of the sparse feature patch. Thus a new consumer can appear without
a feature change. Scores now carry **per-head versions** on the GPU: previously
computed heads remain reusable at the same context version, while requesting a
stale or newly required head computes it before gathering its result.

The original ridge algebra is unchanged:

```
R = active_features @ [high(A), low(A), feature_mask]
score = (R_high + R_low) / original_normalizer(R_count) + original_bias
```

Scopes select which response columns to compute. This does not make a feature
such as `has a Bop child` an exact proof of `ExprIsPure`, and does not merge
bindings having identical feature vectors.

## Packing matters more than simply counting useful columns

Grouping rows solely by identical consumer masks produced smaller logical
matrices but **more padded work**. The GPU uses 8x8 tiles. The final implementation
compares three complete layouts within each sort:

1. Rows grouped by identical missing-head sets.
2. One shared row batch per head.
3. A rectangle covering the union of rows and missing heads.

Choose the smallest actual tile count, breaking ties by arithmetic entries then
bank count. This is an exact choice among three layouts, not globally optimal
layout search. There is no threshold, representative sampling or candidate cap.
The chosen rectangles can compute extra columns when that costs fewer tiles.
All banks share one GEMM dispatch; each retained prediction stays versioned.

## Recorded measurements

One run per changed variant, sequentially. No averages or best-run selection.
Native controls were reused. Setup, exports, audit and Tiger extraction are outside
inference time. Device timers are nested in provider/round-trip timers.

| Revision | All same-sort heads | Consumer heads | Interpretation |
|---|---:|---:|---|
| Exact-mask groups | 14.204 s | 14.638 s | Padding increased; rejected layout |
| Packed groups, ledger in both controls | 14.701 s | 15.393 s | Less matrix work, no net benefit |
| Final opt-in ledger | 16.316 s | 14.794 s | Faster single pair; insufficient attribution |

The final pair uses the same binary, model and workers, differing only in scope
mode. Its 1.52-second wall-time improvement is **not explained** by the much
smaller predictor savings: native RHS time also moves from 3.983 to 3.599 seconds,
and ordinary GPU round-trip time from 4.253 to 3.897 seconds, although the complete
binding population is unchanged. These timers overlap other phase timers and
must not be added as independent costs. Earlier changed-version pairs go in the
opposite direction. We retain all attempts and do not claim a stable 1.10x gain.

The repeatable structural result on the final workload is:

| Work / cost | All same-sort heads | Packed consumer heads |
|---|---:|---:|
| Nonconstant head evaluations | 8,769 | 5,353 |
| Matrix response entries | 26,307 | 16,059 |
| Padded matrix entries | 36,160 | 27,264 |
| Matrix banks | 41 | 62 |
| Resident contexts | 438 | 438 |
| Feature patches / assignments | 549 / 5,582 | 549 / 5,582 |
| New score requests | 827 | 827 |
| Exact CPU residual reads | 708,814 | 708,814 |
| Predictor upload bytes | 64,164 | 84,216 |
| Predictor device time | 3.591 ms | 2.925 ms |
| Predictor round trips | 61.972 ms | 62.782 ms |
| Provider maintenance + query time | 1.427 s | 1.343 s |

Head evaluations include work recomputed inside a packed rectangle. Unused-head
counters report retained/discarded versions separately; do not interpret every
unrequested evaluation as a reusable future result. The consumer ledger has
21,027 live typed contexts, 185,558 distinct context/head connections and 586,446
anchor supports at the final snapshot. It has no row-times-plan preallocation,
but its maintenance and metadata upload are still costs. Resident buffers use
365,568 bytes; per-head versions add storage compared with the earlier backend.

The predictor alone occupies only a few milliseconds. Expanding **exact marker
contracts and correlated binding payloads**, so that more original queries avoid
structural search, has greater potential than optimizing these predictions alone.
Examples worth targeting next are shared ContextOf/type/purity tests, typed alias
conditions, and numerical guards over published function values. An existence
bit for Expr-size is not a value or a proof of `Expr-size > k`. New exact contracts
must retain original witnesses, function versions and Run frontiers; no such
extra condition coverage is claimed by this change.

## Verification and reproduction

- 33 Rust tests pass, including duplicate supports, sort separation, new demand,
  late facts, function replacement, union, subsume and correlated witnesses.
- 712 resident GPU scores checked against the unchanged FP16-coordinate model;
  maximum absolute error 1.1920928955078125e-7, zero threshold disagreements.
  Additional masked-feature checks, growth, partial refresh, late-head refill,
  all-sort fallback and stale-version rejection pass with Metal validation on.
- 156,426 independent native checks of supplied query bindings pass. This is not
  an audit of every ordinary GPU fallback query.
- Final graphs agree with the same-submit native control: 699,068 original rows,
  524,631 mapped eclasses, including values, equivalence and visibility.
- Both timed variants emit 2,768,828 complete original bindings. Actual Tiger
  extraction agrees on all 29 functions. Pristine/fused-native equivalence is not
  newly claimed.

Build with `build.py` as in RESIDENT_RESULTS.md. Use the unchanged lowered model,
ordinary query worker, saved Pass-3 source, joint services and boundary program.
Run the existing `run.py` with `--matrix-layout resident --resident-scope sort`
for the control, then `--resident-scope consumers --compare-consumer-scope CONTROL`
for the candidate. Keep `--coordinates markers --joint-sat --boundary-programs
PROGRAM --model MODEL --query-worker WORKER --reuse-native NATIVE_CONTROL` on both.
Use `--lanes gpu,audit --extract` on the candidate. Default runtime controls do
not include the new subscription ledger.

`consumer-results.json` records exact commands/input identities through the lane
provenance, source and worker hashes, all attempts, per-rule depth distributions,
numerical checks, final graph and extraction checks. The final raw outputs are in
`out/ridge-consumer-final{,-sort}`. Regenerable full graph exports from the first
rejected attempt were removed to recover disk space; its reports and logs remain.
