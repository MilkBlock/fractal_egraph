# Analyze-spine coordinates instead of opaque key memorization

**On a fixed new-binding holdout, replacing opaque-ID coordinates with observed
spine coordinates improves recall from 66.59% to 95.31%, with the same regression
solver, width, labels and threshold.** Precision improves from 92.26% to 96.15%.
The predictor is still speculative. This is not live Cell recognition, a closure
certificate, complete matching, or an end-to-end egglog acceleration.

## Inputs and the leakage boundary

The shape vocabulary comes from **121 completed Analyze observations** in the
saved eggcc run, specifically their `trigger-state.json` constructor graphs.
They provide 94 shape tokens after filtering; 80 occur in multiple Cells.
Every input file has a recorded hash. These exports include observation-only
states; completion is not upgraded into a semantic saturation certificate.

The target graph is the same saved canonical Pass-3 snapshot and native-verified
452-query class used by `query_response_operator` and `superposed_key_memory`.
Only 282 queries have nonempty anchor tables. We reuse the complete projected
answer digest rather than rerunning an unchanged native control.

**An initial attempt was invalid and is excluded.** Native lowering reifies a
relation as a constructor returning `___NameSort` (`egglog/src/ast/desugar.rs`,
`desugar_relation`). Reading every printed constructor therefore admits target
analysis labels such as `ExprIsPure` and `ContextOf` through their incoming
roles. The accepted reader excludes these generated sorts and generated fresh
tables. A regression tests this exact mechanism. The preliminary 97% recall
figure from the contaminated attempt must not be used.

Accepted inputs for coordinates are:

* Ordinary constructor names, declared types and actual committed connections.
* All constructor alternatives in an eclass, including non-root positions.
* Ordered argument roles, one/two-edge spine paths observed by Analyze, and
  equality of argument slots. Opaque RipenInput names/indices are not features.
* Ordered positions in compound lookup keys and exact aliases between those
  positions. Eclass ID numbers themselves do not appear in the coordinates.
* A declared small literal encoding: type, integer sign/bit length, -1/0/1,
  and booleans. No opaque-identity bits masquerade as scalar features.

The extra parameter variant adds intrinsic zero-arity constructor identities
and literal/enum arguments at observed spine ports. It is reported separately;
it does not materially improve the main candidate metric.

Target relations/functions are read to produce supervised labels and the oracle,
**not** to produce features. Constructor-target queries can be directly exposed
by constructor features, so results excluding every constructor-target channel
are also reported. This is a shallow ordinary-structure control as well as a
way to test deeper, Analyze-derived paths.

## Fixed matrix representation

For a typed key, assemble structural tokens such as:

```
slot 0: Expr
slot 0: output of Bop
slot 0: Bop.argument[2] can be Const
slot 0: input at Get.argument[0]
slot 1: Type
alias(slot 0, slot 2)     # only when actually equal
```

Each token has a fixed signed 256-coordinate code. Sum those codes and divide
by a power of two near the square root of the token count. This yields exactly
representable FP16 coordinates and is invariant to bijective eclass-ID renaming.
Ordered slots and aliases remain distinguishable. Equal feature vectors do NOT
mean equal terms, equal Cells or equal complete bindings.

One model per key schema predicts all corresponding conditions:

```
scores = bias + Phi(structural_tokens) @ W / 256
```

The Expr schema has 43 output channels. Weighted ridge regression with fixed
regularization 1 fits the same shared matrix, retaining conflicting labels for
identical signatures rather than selecting one representative. The weights are
stored as FP16; scores are thresholded at 0.5. This is a numerical approximation
with CPU verification/fallback, not a rule proof or calibrated probability.

## Results after removing label leakage

All rows below use the SAME 23,227 withheld keys, the same 40,111 positive
rule/anchor pairs depending on those keys, and the same false-pair population:

| Coordinates / control | Precision | Recall | Missed true pairs |
|---|---:|---:|---:|
| Constant response prior | 91.13% | 68.49% | 12,638 |
| Opaque ID codes, matched solver | 92.26% | 66.59% | 13,401 |
| Constructor heads only | 96.02% | 91.61% | 3,367 |
| One-edge spines + roles + aliases | 96.17% | 95.30% | 1,887 |
| Two-edge spines + roles + aliases | **96.15%** | **95.31%** | **1,883** |
| Two-edge + enum/literal parameter specialization | 96.00% | 95.32% | 1,878 |

The extra parameters trade five recovered positives for 62 additional false
positives compared with the two-edge model, so they are not presented as a win.
One-edge coordinates capture almost all of the useful gain with less extraction
work. The GPU packet uses the predeclared two-edge configuration, not a best-of
timing/model selection.

On withheld unique key/channel tests excluding all constructor-target channels,
the two-edge model has **98.62% precision / 96.87% recall**, compared with
**98.53% / 76.90%** for its prior-only control. This separates derived-analysis
prediction from simply testing whether a constructor is present.

A second split withholds entire structural signatures. For the plain two-edge
model, affected rule/anchor recall is **84.44%**, precision **99.61%**. This is
harder than new bindings of familiar shapes. Signature-held-out populations
change with the feature definition, so those columns must NOT be compared
across depths as if they were a fixed test set. All results and populations are
retained in `results.json`.

Held-out keys still have their current constructor topology available. Labels
are withheld, not the input graph. This is transductive evaluation on one saved
graph, not an unseen-program, unseen-pass or future-schedule guarantee.

## GPU and storage

The existing grouped matrix worker now accepts `SPINE001` token-bag packets.
It generates the dense coordinates on device, executes the shared GEMMs, and
adds biases. Original `SKETCH01` packets retain their original fast encoder.

For the accepted two-edge packet:

* **59,856 bytes** of unpruned model weights/biases over 20 key schemas.
* 75,035 input probe keys map to 1,263 distinct query signatures.
* GPU emits 14,649 shared condition scores. Reconstruction to original keys and
  then to rule/anchor candidates is outside the timed GPU section.
* Only six blocks have nonconstant weights: 332 rows and 6,188,544 useful matrix
  FLOPs. Constant blocks use their bias; they do not earn matrix-compute credit.
* GPU scores agree with the CPU FP16 reference, with zero threshold differences.

The retained final-worker run took approximately **0.168 ms device time** and
0.479 ms host encode/submit/wait. This is a small standalone Apple matrix kernel
workload, NOT NVIDIA Tensor Core evidence or a claim of neural-network-like
utilization. It is not directly comparable to the previous 0.5105 ms result,
which expanded over a million condition outputs rather than shared signatures.

GPU buffers in this run:

| Allocation | Bytes |
|---|---:|
| Packed weights, biases and block routing | 46,772 |
| Current structural token bags + offsets | 40,032 |
| Temporary dense coordinates | 169,984 |
| Shared score outputs | 58,596 |

This excludes CPU graph storage, feature caches, original-key-to-signature
routing and the verification/fallback machinery. The smaller model is not a
measurement of total egraph memory compression.

CPU extraction is still material work: approximately 1.47 s for one-edge and
2.56 s for two-edge features in this Python prototype, scanning 280,104 ordinary
constructor rows. Offline model fit/prediction is about 0.08 s. No runtime event
publisher or incremental union invalidation has replaced this graph scan yet.

The first shared GPU encoder regressed the old opaque-key path. That development
version is excluded: separate scalar-key and token-bag kernels restore the old
path. A paired single-run compatibility check of the unchanged old packet gave
1.153 ms (old binary) vs 1.148 ms (new), identical responses and buffer sizes.
This checks accidental regression, not a speedup. Historical absolute GPU times
vary, so the current paired control is retained alongside source identities.

## Reproduce

Use Python with NumPy and the same recorded inputs. `SOURCE` is the flattened
native Pass-3 source. `ANALYZE_QUEUE` identifies the completed observed Cells.
The native oracle report must contain the complete answer digest.

```sh
python -m unittest discover -s experiments/structural_key_memory -p 'test_*.py' -v
python experiments/structural_key_memory/train.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --source "$SOURCE" --analyze-queue "$ANALYZE_QUEUE" --native-report "$NATIVE_REPORT" --output out/structural-keys
# Extra controls do not repeat the unchanged main variants:
python experiments/structural_key_memory/train.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --source "$SOURCE" --analyze-queue "$ANALYZE_QUEUE" --native-report "$NATIVE_REPORT" --random-control --output out/structural-random
python experiments/structural_key_memory/train.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --source "$SOURCE" --analyze-queue "$ANALYZE_QUEUE" --native-report "$NATIVE_REPORT" --parameters-only --output out/structural-parameters
/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc -O -sdk /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk experiments/superposed_key_memory/gpu.swift -o out/structural-keys/worker
out/structural-keys/worker out/structural-keys/input.bin out/structural-keys/reference.bin out/structural-keys/gpu.json
```

Tests cover alpha-renaming, key order, alias changes, all constructor alternatives,
non-root ports, analysis-label exclusion, native relation desugaring, vocabulary
restrictions and specialization of enum/literal parameters. Generated packets,
compiled workers and caches stay untracked. Native tracing, matcher, updates and
effect submission are unchanged.
