# Polyspine: full-scale real Pass-3 local updates

The unchanged source schedule executed **10,121 native Runs**. All **182 saturation visits** of four declared families were observed. No copied toy graph or positive-only sampling is used.

Dataset: **3,993,271 local stage observations**, **3,099,846 eligible predictions**, **23,659 distinct final union lineages**. The group split holds out **4,730 lineages**. Repeated stages are observations, not independent programs.

## What was actually expanded

| Stage | Sat visits | Eligible observations | Answer gains/updates | Updates to existing answers | Outside pre-stage domain |
|---|---:|---:|---:|---:|---:|
| type-analysis | 85 | 1,865,187 | 24,636 | 0 | 49 |
| always-run | 49 | 830,189 | 3,206 | 0 | 2,404 |
| is-resolved | 36 | 141,623 | 23,230 | 0 | 0 |
| terms | 12 | 262,847 | 11,880 | 3,473 | 0 |

Targets are HasType payload additions, new ExprIsPure / ExprIsResolved answers, and ExtractedExpr payload changes (including one-level TCPair candidate/cost content). These are projected answer updates, **not complete rule matches, committed mutation counts or extraction quality**.

## Union-lineage holdout across the same program

| Stage | Train / test observations | Test positives | Test lineages | Entry P/R | Polyspine P/R | Residual-8 P/R | Versioned-8 P/R |
|---|---:|---:|---:|---:|---:|---:|---:|
| type-analysis | 1,488,973 / 376,214 | 4,988 | 4,730 | 96.7 / 100.0 | 96.8 / 100.0 | 96.8 / 100.0 | 96.8 / 100.0 |
| always-run | 662,813 / 167,376 | 669 | 4,276 | 93.9 / 34.2 | 94.0 / 37.4 | 96.2 / 49.8 | 96.0 / 49.8 |
| is-resolved | 113,340 / 28,283 | 4,681 | 4,730 | 100.0 / 99.9 | 100.0 / 99.9 | 100.0 / 99.9 | 100.0 / 99.9 |
| terms | 209,774 / 53,073 | 2,309 | 4,730 | 98.8 / 3.5 | 92.7 / 42.4 | 92.7 / 42.4 | 94.5 / 43.0 |

P/R are precision and recall percentages at the fixed 0.5 threshold. No best-threshold selection. An em dash precision means there were no positive proposals.

## Chronological holdout: cycles 6–11

| Stage | Train / test observations | Test positives | Test lineages | Entry P/R | Polyspine P/R | Residual-8 P/R | Versioned-8 P/R |
|---|---:|---:|---:|---:|---:|---:|---:|
| type-analysis | 855,255 / 1,009,932 | 516 | 23,659 | 98.1 / 98.3 | 97.7 / 100.0 | 97.7 / 100.0 | 97.7 / 100.0 |
| always-run | 392,494 / 437,695 | 72 | 18,326 | 68.0 / 23.6 | 76.0 / 26.4 | 96.7 / 81.9 | 96.7 / 81.9 |
| is-resolved | 56,099 / 85,524 | 550 | 5,023 | 86.8 / 99.3 | 86.9 / 99.3 | 87.5 / 99.3 | 87.5 / 99.3 |
| terms | 121,727 / 141,120 | 1,151 | 23,659 | 7.5 / 4.9 | 23.0 / 36.2 | 23.0 / 36.2 | 20.3 / 38.0 |

P/R are precision and recall percentages at the fixed 0.5 threshold. No best-threshold selection. An em dash precision means there were no positive proposals.

## Interpretation

The larger cold-lineage purity test recovers 333/669 positives. The earlier 45/47 warm-state result did not cover this harder population.

Extraction-state updates reach 94.5 / 43.0 P/R in lineage holdout, but only 20.3 / 38.0 in the later-time test. This gap must not be hidden by reporting only the stronger split.

The resolved structural baseline is already very strong; its accuracy cannot be credited to Polyspine or residual propagation. The clearest additional learned benefit is cross-interface information for extraction updates, although many updates are still missed.

Current function features retain returned constructor heads and answer counts, not full numeric candidate costs and update programs. The four version flags also do not encode arbitrary structural/union transitions. These are concrete representation gaps; this experiment does not identify a single causal explanation for every error.

No predicted negative is used to skip native work. These results do not justify removing exact verification or fallback.

## Complete ablation counts

| Split | Stage | Variant | TP | FP | FN | TN |
|---|---|---|---:|---:|---:|---:|
| time | type-analysis | shape | 11 | 59 | 505 | 1,009,357 |
| time | type-analysis | entry | 507 | 10 | 9 | 1,009,406 |
| time | type-analysis | polyspine | 516 | 12 | 0 | 1,009,404 |
| time | type-analysis | residual1 | 516 | 12 | 0 | 1,009,404 |
| time | type-analysis | residual8 | 516 | 12 | 0 | 1,009,404 |
| time | type-analysis | versioned8 | 516 | 12 | 0 | 1,009,404 |
| time | type-analysis | shuffled8 | 516 | 12 | 0 | 1,009,404 |
| time | always-run | shape | 16 | 14 | 56 | 437,609 |
| time | always-run | entry | 17 | 8 | 55 | 437,615 |
| time | always-run | polyspine | 19 | 6 | 53 | 437,617 |
| time | always-run | residual1 | 49 | 0 | 23 | 437,623 |
| time | always-run | residual8 | 59 | 2 | 13 | 437,621 |
| time | always-run | versioned8 | 59 | 2 | 13 | 437,621 |
| time | always-run | shuffled8 | 19 | 6 | 53 | 437,617 |
| time | is-resolved | shape | 546 | 83 | 4 | 84,891 |
| time | is-resolved | entry | 546 | 83 | 4 | 84,891 |
| time | is-resolved | polyspine | 546 | 82 | 4 | 84,892 |
| time | is-resolved | residual1 | 546 | 78 | 4 | 84,896 |
| time | is-resolved | residual8 | 546 | 78 | 4 | 84,896 |
| time | is-resolved | versioned8 | 546 | 78 | 4 | 84,896 |
| time | is-resolved | shuffled8 | 546 | 82 | 4 | 84,892 |
| time | terms | shape | 39 | 1,401 | 1,112 | 138,568 |
| time | terms | entry | 56 | 690 | 1,095 | 139,279 |
| time | terms | polyspine | 417 | 1,397 | 734 | 138,572 |
| time | terms | residual1 | 417 | 1,397 | 734 | 138,572 |
| time | terms | residual8 | 417 | 1,397 | 734 | 138,572 |
| time | terms | versioned8 | 437 | 1,714 | 714 | 138,255 |
| time | terms | shuffled8 | 417 | 1,397 | 734 | 138,572 |
| lineage | type-analysis | shape | 1,943 | 51 | 3,045 | 371,175 |
| lineage | type-analysis | entry | 4,988 | 170 | 0 | 371,056 |
| lineage | type-analysis | polyspine | 4,987 | 167 | 1 | 371,059 |
| lineage | type-analysis | residual1 | 4,987 | 167 | 1 | 371,059 |
| lineage | type-analysis | residual8 | 4,987 | 167 | 1 | 371,059 |
| lineage | type-analysis | versioned8 | 4,987 | 167 | 1 | 371,059 |
| lineage | type-analysis | shuffled8 | 4,987 | 167 | 1 | 371,059 |
| lineage | always-run | shape | 229 | 17 | 440 | 166,690 |
| lineage | always-run | entry | 229 | 15 | 440 | 166,692 |
| lineage | always-run | polyspine | 250 | 16 | 419 | 166,691 |
| lineage | always-run | residual1 | 276 | 14 | 393 | 166,693 |
| lineage | always-run | residual8 | 333 | 13 | 336 | 166,694 |
| lineage | always-run | versioned8 | 333 | 14 | 336 | 166,693 |
| lineage | always-run | shuffled8 | 250 | 16 | 419 | 166,691 |
| lineage | is-resolved | shape | 4,677 | 0 | 4 | 23,602 |
| lineage | is-resolved | entry | 4,677 | 0 | 4 | 23,602 |
| lineage | is-resolved | polyspine | 4,677 | 0 | 4 | 23,602 |
| lineage | is-resolved | residual1 | 4,676 | 0 | 5 | 23,602 |
| lineage | is-resolved | residual8 | 4,676 | 0 | 5 | 23,602 |
| lineage | is-resolved | versioned8 | 4,676 | 0 | 5 | 23,602 |
| lineage | is-resolved | shuffled8 | 4,677 | 0 | 4 | 23,602 |
| lineage | terms | shape | 3 | 0 | 2,306 | 50,764 |
| lineage | terms | entry | 81 | 1 | 2,228 | 50,763 |
| lineage | terms | polyspine | 980 | 77 | 1,329 | 50,687 |
| lineage | terms | residual1 | 980 | 77 | 1,329 | 50,687 |
| lineage | terms | residual8 | 980 | 77 | 1,329 | 50,687 |
| lineage | terms | versioned8 | 994 | 58 | 1,315 | 50,706 |
| lineage | terms | shuffled8 | 980 | 77 | 1,329 | 50,687 |

## Direct projection is a separate control

| Split | Stage | TP | FP | FN | TN |
|---|---|---:|---:|---:|---:|
| time | always-run | 59 | 0 | 13 | 437,623 |
| lineage | always-run | 150 | 0 | 519 | 166,707 |
| time | is-resolved | 546 | 0 | 4 | 84,974 |
| lineage | is-resolved | 1,356 | 0 | 3,325 | 23,602 |

Only purity and resolved currently have the admitted raw-row GPU propagation. The type and extraction heads use observed entry interfaces and learned versions; they must not inherit propagation guarantees from the other two heads. Positive finite-depth propagation and ridge extrapolation are reported separately. Unknown/negative scores do not authorize discarding matches.

## Split and leakage checks

- Every historic root joined by native union belongs to exactly one lineage split. Train/test lineage intersections are empty.
- Future union lineage is used only for grouping the split, not to supply a feature or an earlier alias.
- Version features retain only earlier pre-stage shapes, interfaces and answer counts. Current-stage labels are never read by feature construction.
- The lineage split is transductive within this same program; it is not held-out programs, whole functions or disjoint neighborhoods.
- The time split allows previously seen roots; its results are not silently substituted for unseen-lineage results.
- New output keys outside the pre-stage domain are counted above and remain uncovered.

## Work and costs

85 GPU projection frames were checked against an independent CPU reference. Ridge scoring checked 144,180 outputs, maximum error 4.76837158203125e-07, threshold differences 0.

The fit retains 168,388 distinct vectors and 44 distinct fitted models. Repeated coordinates retain their full positive/negative multiplicities; contradictory labels are not collapsed to a representative.

Diagnostic observation: 1217.275 s; feature preparation: 423.194 s; stream aggregation plus independent projection audit: 323.375 s; ridge fit/score on CPU: 2.191 s. Compact observer traffic: 45.50 MiB. GPU score kernel time: 4.501 ms.

The observer scans selected tables and sends lossless differences. It is not a production event journal. CPU feature construction, stored projection inputs and offline fitting remain real costs. There is no end-to-end speedup or Tensor Core utilization claim.

## Provenance

- Source SHA-256: `c727cdf724b020ae585c4051fda36b66576c1ce4252a4d9d59fbb7522d0c7d4f`
- Native oracle SHA-256: `8fad3df4996a7b257d24365a355e783f1a6dd8c41b3f8bba1266826b83d56278`
- Dataset SHA-256: `660328ed751c642beff4f961788af40309a0a4a32be52101e32b17d010423f71`
- Score worker SHA-256: `b6bfc1df97e69086d0f81ec0e9efa4eb5d2d19d071f053defe3167d9a02f1cb5`
- Full dataset and untracked binary artifacts: `out/polyspine-scale-data`.
- Full score results: `out/polyspine-scale-fit/results.json`.
- A subsequent container-inner lineage fix in the read-only observer was tested separately; the measured capture binary is retained with the fingerprint above.
- 26 regressions passed, including actual GPU checks and native union/set/subsume/container observation.
