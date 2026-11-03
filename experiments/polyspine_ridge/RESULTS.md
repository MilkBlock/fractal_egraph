# Polyspine ridge: Raytrace Pass 3

**A small gain on purity classification, but worse recall on newly enabled answers.**

This is an offline, causally sampled representation test. It does not accelerate the production runtime.

Native replay checked all **10,121 Run update flags** against the saved verified trace. We sampled **48 Saturate boundaries** across 12 outer cycles; cycles 0–5 train and 6–11 test. Every variant uses the same rows, ridge penalty 1, 512 coordinates, FP16 weights and threshold 0.5.

Total observations: **1,013,276** across four targets. These are repeated Expr/stage observations, not unique Cells or native rule matches.

## always-run / ExprIsPure

| Coordinates | Precision | Recall | False positives | Missed positives | Raw ridge Brier |
|---|---:|---:|---:|---:|---:|
| structure | 94.44% | 83.82% | 1,542 | 5,059 | 0.04659 |
| stage | 94.44% | 83.82% | 1,542 | 5,059 | 0.04659 |
| interfaces | 94.50% | 84.09% | 1,531 | 4,973 | 0.04678 |
| polyspine | 94.88% | 84.06% | 1,419 | 4,984 | 0.04484 |
| shuffled_seams | 94.31% | 83.93% | 1,582 | 5,023 | 0.04658 |

**Unknown at stage entry:** 47 positive answers are newly enabled in the held-out purity population. The remaining candidates are negative. Do not use total accuracy or already-present answers to hide this small, difficult subset.

| Coordinates | Newly enabled answers recovered | False proposals | Precision | Recall |
|---|---:|---:|---:|---:|
| structure | 29/47 | 1,542 | 1.85% | 61.70% |
| stage | 29/47 | 1,542 | 1.85% | 61.70% |
| interfaces | 29/47 | 1,531 | 1.86% | 61.70% |
| polyspine | 19/47 | 1,419 | 1.32% | 40.43% |
| shuffled_seams | 29/47 | 1,582 | 1.80% | 61.70% |

Relative to local interfaces, correct Polyspine wiring fixes 534 held-out decisions and breaks 433 (101 net). Relative to shuffled wiring, it fixes 646 held-out decisions and breaks 444 (202 net). These are paired decisions on the same rows, not independent programs or a significance claim.

## Other stage families

| Stage / target | Structure precision | Structure recall | Polyspine change |
|---|---:|---:|---|
| type-analysis / HasType | 99.99% | 100.00% | None: no admitted Expr-to-interface seam |
| is-resolved / ExprIsResolved | 99.99% | 100.00% | None: no admitted Expr-to-interface seam |
| terms / ExtractedExpr | 93.75% | 72.93% | None: no admitted Expr-to-interface seam |

The current read-set and target masking produce useful cross-stage features only in always-run (mostly HasType/HasArgType interfaces supplied by earlier stages). The others are explicit negative controls, not evidence that Polyspine works or fails for their complete rule systems. In particular, the terms task needs candidate/parameter and recurrent-state features beyond this one-edge interface view.

Stage labels alone change no decision here: the model already has a head per target, and the sampled previous-stage labels are nearly fixed. This does not establish that schedule context is generally useless.

## What this result diagnoses

- Connected interfaces contain a little information: the correctly wired model beats the same bags shuffled among roots.
- Newly enabled purity recovery changes from 29/47 to 19/47. This must be weighed separately from old answers.
- The Polyspine token signatures still mix positive and negative answers: even a test-oracle majority lookup would make 3,100 purity errors (versus 3,460 for structure). This is a descriptive information-loss bound, not a trained model or an independent benefit metric; random signature splitting can also reduce it.
- We encode observed one-edge overlaps and possible producing rulesets, not complete multi-stage derivation programs or witnessed per-row rule histories. Missing recursive context, correlated alternatives and parameter updates remain plausible causes; this experiment does not separately identify their contributions.
- Do not compare these percentages directly with the older final-snapshot new-binding split: labels and holdout are different.

## Computation and limits

The existing Apple matrix worker evaluated all five variants in **5.867 ms** device time (6.131 ms encode/submit/wait). It emitted 261,360 scores, including unused heads. FP16/FP32 score comparison: maximum error **0**, **0** threshold disagreements.

Offline CPU feature extraction: **42.757 s**; packing: **16.430 s**; snapshot observation: **346.537 s**. These costs are real. No end-to-end acceleration or GPU utilization claim is made.

The worker is reused unchanged; generic “Analyze-derived” captions inside its raw gpu.json describe its historical packet format, not this dataset's provenance. Here the evidence is the native pre-stage snapshot and typed rules. Final predicate truth is never used to construct features.

## Provenance

- Source SHA-256: `c727cdf724b020ae585c4051fda36b66576c1ce4252a4d9d59fbb7522d0c7d4f`
- Native oracle SHA-256: `55b78ec3c6249184a12335d0f7775b378e600a2fd25161bb4855feb542911e46`
- Dataset SHA-256: `65efecab64ce508c671e822c53659fe8886e52088845a0300ebe99e9a9c0f34a`
- Worker SHA-256: `52f80836f690c030d9ac8f9ce4d42a93f88a333ab125ef5a3b365316e36af9fb`
- Full local result: `/Users/mineralsteins/.codex/worktrees/d569/egg_layout/out/polyspine-pass3-data/../polyspine-pass3-ridge/results.json`

Nine regressions cover connected versus disconnected contexts, argument position, alpha renaming, target/private-sort masking, visibility, union label mapping, nested scheduling, ridge aggregation and conflicting labels.
