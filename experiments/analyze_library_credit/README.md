# What the automatic Analyze library currently contributes

This is the actual complete saved raytrace Pass 3 in the native/GPU runtime,
not a small matcher benchmark. Every original rule, schedule, native action,
merge/rebuild and the existing source-compiled joint observer is retained.
The only treatment is a frozen ordinary Analyze catalog. No raytrace fact or
match is added to training while testing.

The result is negative: enabling this library costs **1.060620 s (+7.705%)**
in the single paired run. This does not show that arbitrary RipSatCell reuse
is ineffective: it measures five certified Cells learned from the eggcc
If/Single subset, not a comprehensive Pass-3 library. Much of this library's
interface is irrelevant to this pass.

## Two different sources of execution plans

The existing 31 parameterized programs are compiled from typed source rules.
They choose a root, follow key/output reads, subscribe to changed keys and
cache intermediate bindings. The joint proof group adds separately checked
closure/monotonicity contracts. Their usefulness does not demonstrate that
ordinary Analyze automatically discovered those contracts.

`analyze --bake-transfers` and `analyze-transfer` can export the supported terms
family, using actual history as evidence and complete typed queries to check
wiring. This is not a general exporter for every Cell or scalar rule; v1 rejects
scalar programs. It is also distinct from the positive structural catalog.

The structural loader already accepted `catalog/catalog.json` and completed
states. Before this change the full Pass-3 entry point never supplied that
catalog. This experiment connects it, using existing projection discovery at
root and non-root positions. Each reused read has a fully determined native
key. Native row generations and key watches validate its current binding;
unknown/changed instances retain complete matching. No absent catalog entry is
used as a proof of impossibility, and no whole saturation is skipped by these
projections.

## Library and runtime coverage

`library/` preserves the existing ordinary Analyze output, including its exact
training source, original provenance and five positive constructor/equality
states. Catalog/state JSON totals **179,535 bytes**. Bake is historical and not
rerun or counted as zero cost. `library-provenance.json` records the source and
checksums. The input is the existing full `pass-3-flat.egg` snapshot.

Five states compile into **6 shared projections used by 13 of 547 rules**.
81 queries with computed primitives keep the ordinary GPU path; this does not
exclude their execution. Other unsupported/nonfunctional projections also retain
the complete query. In particular, reverse fibers with multiple results are
not converted into a single cached row.

Of those 13 consumers, 11 have every GPU request pruned by the generic empty-table
check. Only two interval-analysis rules produce bindings (46 each):

```egg
(rule ((= lhs (Bop (LessThan) a b))
       (= (IntB ha) (hi-bound a))
       (= (IntB lb) (lo-bound b)))
      ((set (lo-bound lhs) (BoolB (bool-< ha lb)))))
```

The other rule computes the upper bound. The reusable internal piece is the
small `Bop`/`LessThan` connection; the changing bound reads remain outside it.
This is a concrete example of structural coverage yielding very little useful
matching reuse. It is not evidence of broad whole-Cell transition coverage.

## Controlled timing

Same Rust binary, worker, input, schedule, source-compiled observer and generic
GPU optimizations. Each configuration runs once, without diagnostic counters.
Native/CPU graph controls are reused; the three GPU runs and exact graph checks
are fresh. No historical GPU timing is used in these differences.

| Mode | Setup s | Inference s | GPU device s | Final allocated GPU MiB |
|---|---:|---:|---:|---:|
| No Analyze library | 4.539090 | 13.765348 | 2.765187 | 458.82 |
| Library-guided order/backend only | 30.030826 | 14.508555 | 2.906858 | 459.08 |
| Library projections and subscriptions | 38.522557 | 14.825968 | 3.162374 | 464.47 |

Inference includes runtime validation, subscriptions, transport and native
commits. Setup includes projection compilation and worker initialization and
is excluded from inference; it is not negligible. Bake, extraction and graph
serialization are separate. GPU allocations are not peak RSS.

Library-guided order/backend costs 0.743207 s relative to the ordinary path;
turning on actual projection reuse adds 0.317413 s. These are ordered conditional
contrasts, not independently additive semantic credits. The plan-only control
keeps the same fragment-guided order and query backend. The existing joint
proofs and 30,566 CPU query answers / 1,918,606 bindings stay common to all lanes.

## Work, measured separately from time

| Work counter | Order only | Projection reuse |
|---|---:|---:|
| Ordinary indexed Join probes | 24,735,017 | 24,734,577 |
| Explicit certified-segment reads skipped | 0 | 76 |
| Certification attempts / keyed probes | 0 / 0 | 12,633 / 12,615 |
| Port residual probes | 0 | 272 |
| Dependency watch entries checked | 0 | 273,075 |
| Missing-key waits | 0 | 12,406 |
| Published ready ports | 0 | 227 |
| Port-supplied complete bindings | 0 | 60 |
| Final live interface instances | 0 | 3,990 |

The raw Join probe difference is 440; the explicit skip counter is only one
path and excludes changes in port ownership. Extra certification and port work
must remain visible. These counts are neither unique nodes nor seconds. The
ordinary probe counter has its existing packet-retry accounting; do not assert
a conservation equation with the other counters.

Residual compact-query depth distributions are identical: 8,535,634 counted
starts and 42,244,100 live candidate visits in both diagnostic lanes. The 13
projection queries use the existing certificate executor, which lacks complete
per-depth row counters. `diagnostics.json` explicitly limits its depth curves to
the remaining compact queries; they are not whole-workload curves. No timing
claim uses these instrumented runs.

## Runtime changes and checks

- Full-runtime `new_full_configured` accepts the existing Analyze catalog.
- Structural projections coexist with computed queries, which fall back per
  query. Unaffected queries keep compact bindings and generic refutation;
  loading a library no longer globally disables those optimizations.
- Table-anchor dispatch now executes the ready-port lane as well. Mixed-program
  Cell/port lookups share exact raw-i64 index hashing with ordinary queries.
- Benchmark cache identity includes all catalog/state file hashes. Plan-only
  and reuse runs must load the identical library. Reference lanes do not load it.

All three timing and two diagnostic final graphs agree with the saved
same-submit CPU's complete facts, values, equality and visibility. The existing
fused-native mismatch is preserved; this is not a pristine comparison. No new
Tiger extraction was run. A fresh full native-frontier audit passes **478,136**
queries. Full-GPU regressions include mixed structural/numeric queries, union,
subsume and subsequent insertions. See `verification.json` for checks.

## Reproduce using the existing skill

```sh
SKILL=skills/cell-assisted-egglog-bench/scripts/bench.py
# Use the same worker and --reuse-controls directory for all three runs.
python3 "$SKILL" run --all-rules --joint-sat --no-extract --output out/new-none
python3 "$SKILL" run --all-rules --joint-sat --no-extract \
  --analysis experiments/analyze_library_credit/library --analysis-plan-only \
  --output out/new-plan
python3 "$SKILL" run --all-rules --joint-sat --no-extract \
  --analysis experiments/analyze_library_credit/library --output out/new-reuse
python3 "$SKILL" analyze-credit --baseline out/new-none \
  --plan-only out/new-plan --candidate out/new-reuse --output out/new-credit.json
```

Actual commands, binary/worker/input/library fingerprints and measured values
are retained in `results.json`. `diagnostics.json` adds the two separately
instrumented runs and each affected original rule. `--profile-plan` and
`--profile-candidate` attach those saved runs without rerunning a benchmark.

To make this a general Analyze-to-runtime compiler, the next missing operation
is extracting complete typed, versioned query/transfer interfaces for a chosen
consumer and schedule environment, including numerical guards and reverse
multi-candidate reads. A historical positive Cell alone does not justify
removing those reads or prove a whole closure valid after an external change.
