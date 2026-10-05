# RipSatCell query anatomy

Extends the existing read-only `ripsat_coverage` diagnostic with
`query_anatomy.json`. It consumes the frozen library derived from analyze's
completed observations and runs real eggcc rules in the native engine. It does
not change the default analyze pipeline, optimizer rules, or Free Join.

Previously the diagnostic counted node coverage and whole-match coverage. This
addition records every connected pair of successful physical reads: ordered
columns, sort/literal information, observed aliases, cross-rule reuse, pre-step
Cell co-membership, effective apply association and distinct binding counts per
step. Concrete identities are discarded after each step. Pairs are normalized
under renaming and swapping their two atoms. Distinct literal values and alias
patterns stay distinct. No arbitrary subgraph enumeration or candidate cap.

Reproduce (one held-out evaluation, existing frozen training library):

```sh
cargo build --offline --release --example ripsat_coverage
cargo test --offline --release --example ripsat_coverage
target/release/examples/ripsat_coverage eval /tmp/eggcc-marker-loopfact.egg out/ripsat-coverage-remeasure-merged/train/library.json out/cell-query-anatomy-loopfact
```

The existing `experiments/ripsat_coverage` workflow recreates training libraries
from analyze observations. Each train/eval invocation now writes the extra report.

## One measured held-out run (2026-10-05)

Native traced control final-state equality passed. The frozen library retained
7,402 signatures. 6,788 complete-witness matches produced 695 connected read-pair
shapes; 126 shapes occur in multiple rules. There were 9,559 pair occurrences:

| Coverage of the two reads | Occurrences |
|---|---:|
| At least one common pre-step Cell embedding | 186 |
| Both covered, no common embedding | 336 |
| Exactly one side covered | 3,169 |
| Neither side covered | 5,868 |

Examples (one specific alias/literal shape per row, not all pairs of these tables):

| Tables | Occurrences | Rules | Same Cell | Exactly one side covered |
|---|---:|---:|---:|---:|
| HasType / TupleT | 375 | 6 | 0 | 0 |
| BodyContainsExpr / Get | 270 | 3 | 5 | 223 |
| ExtractedExpr / TCPair | 265 | 6 | 4 | 39 |

There are 8,282 distinct concrete pair bindings summed over steps versus 9,559
occurrences. Their difference is observed repetition only, not avoidable Join
work: plan position, primitive guards, rule semantics and snapshot dependencies
can differ. The second BodyContainsExpr/Get shape has different aliases and is
correctly kept separate in the full report.

## Interpretation and boundaries

This suggests investigating shared boundary query structure, rather than treating
expression coverage as evidence that whole joins can be skipped. However:

* Coverage is positive pattern support, not certified instance membership.
  Uncovered can mean a missing library pattern; it does not prove externality.
* Observed binding aliases are NOT static variable equalities. These signatures
  cannot authorize cross-rule query replacement without inspecting static LHS.
* Only successful reads are recorded. No failed branch denominator, failure
  certificate, pure Join timing or measured acceleration is inferred.
* Union-effect counts describe unions performed BY the observed match, not
  unions that enabled it or invalidated a prior Cell. That dimension is still
  missing and must be measured separately.
* Pairs within one match overlap; their counts cannot be added as saved work.
* Pair traversal is quadratic in LHS read count (plus encoding); this is an
  offline diagnostic, not a new default hot-path tracing requirement.

Next useful dimension: statically align the recurring pair signatures to query
atoms/guards and runtime plan positions, then measure failed probes and incoming
union dependencies on those specific sites. Do not generate ImpossibleMarkers
from this report.
