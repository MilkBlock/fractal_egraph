# Residual errors, nonlinear coordinates, and contextual blocks

**No new variant is recommended as the default.** This follow-up adds useful
representation primitives and diagnostics, but the larger models trade fewer
misses for additional false candidates. All results are standalone predictors
on the same saved graph, not improvements to native egglog execution.

## What was missing

`audit.py` traces failed complete rule/anchor decisions back to individual
projected conditions. In the original development split, the 1,883 missed pairs
include 1,191 failed `ExprIsPure` occurrences, 677 `Select` occurrences and 168
`TernaryOpIsPure` occurrences. One failed candidate can have several explanations,
so these counts must not be added.

An oracle diagnostic also groups equal structural signatures with conflicting
labels. It computes `sum min(positive_count, negative_count)` for each channel.
Any deterministic classifier using only that signature must make at least that
many mistakes on these labeled keys. This diagnostic uses all labels but never
feeds them back into training. Adding a nonlinear function cannot separate
inputs that are already identical.

## Fixed quadratic coordinates

The first new representation is:

```
Phi2 = [Phi, Phi * Phi]         # elementwise square, fixed, no hidden training
score = Phi2 W / 512 + bias
```

Squaring the mixed signed coordinates introduces pair co-occurrences of input
tokens. No extra graph traversal or target-label read is required. A regression
shows it can represent XOR after a fixed mixing of two Boolean inputs, whereas
a linear readout cannot. It still cannot recover erased binding information.

The model grows from 59,856 to 119,248 bytes. On the original split it recovers
43 additional true pairs and adds 15 false candidates. On an additional split,
declared before running the variant, it recovers 49 and adds 29. The gain is
too small to justify doubling the model by itself.

## Finite structural context

`context.py` propagates a finite bitset along constructor arguments having the
same sort as the constructor result:

```
context(v) = local_constructor_tags(v)
             union context(same-sort child 0)
             union context(same-sort child 1) ...
```

Local seeds include ordinary constructor heads and immediate zero-arity enum
arguments. Analysis relations/functions remain excluded. This is a structural
reachability closure, NOT rule saturation or a semantic purity proof. All
constructor alternatives are included. Cross-sort scope backlinks are not
followed, and this restriction is explicit rather than an implicit assumption
of completeness.

The implementation reaches its finite bitset fixed point without an iteration
or candidate budget. On this graph: 116 tags, 151,653 tagged nodes, 279,743 edges,
370,680 successful bitset unions and 812,007 edge visits. Cycles and cross-sort
boundaries have focused tests.

Controls distinguish new local vocabulary from propagation:

* `local`: existing spines plus only local constructor/enum tags.
* `recursive`: pool those tags together with recursively reachable tags.
* `split_recursive`: keep local and recursive context in two separate 256-wide
  blocks, so expanding context does not replace the local coordinate block.
* `typed_mask`: constructor-target output columns retain the original local
  model; analysis channels may use the context block. This is a mask selected
  from declarations, not by choosing favorable individual test rules. Those
  constructor predictions remain approximate, not exact certificates.

## Paired results

Each row is compared within the same split. The two splits have 40,111 and
39,209 true rule/anchor pairs respectively. The extra split is still from the
same graph; it is not external-workload validation.

| Representation | Original precision / recall | Additional precision / recall |
|---|---:|---:|
| Original linear spine | 96.15% / 95.31% | 99.15% / 94.23% |
| Fixed square | 96.12% / 95.41% | 99.08% / 94.36% |
| Extra local tags | 96.20% / 95.79% | 93.84% / 94.85% |
| Pooled recursive tags | 95.88% / 96.46% | 93.62% / 95.63% |
| Separate local/context blocks | 96.01% / 96.86% | 93.63% / 95.94% |
| Declaration-masked context | 95.32% / 96.09% | 92.95% / 95.03% |

The declaration mask also failed a third paired split: baseline 98.64% precision
/ 90.10% recall versus 90.12% / 90.91% for the masked model. Thus it is not a
reliable fix for the false-candidate increase. All trials are retained.

## Why condition accuracy was misleading

False-candidate auditing exposed an important interaction. In one split,
`PtrAdd` was incorrectly predicted to satisfy `bpred-of-type` and `bop-of-type`.
Those mistakes participated in 1,218 and 1,101 false candidate occurrences.
The source program initializes these operator categories explicitly. Another
case predicts the held-out `Select` value as `Write`.

One mistaken key can be consumed by many rule/anchor pairs. Conversely, improving
another condition can expose this mistake: a candidate previously rejected by
an unrelated false negative now passes. Therefore:

* Better per-key accuracy does not imply better complete-candidate precision.
* The existing squared-error objective does not account for consumer fanout or
  the conjunction of several predicted conditions.
* Holding out a key that represents an entire operator symbol can remove all
  positive/negative examples for a tiny vocabulary category. This is explicitly
  reported rather than relabeled as a generalization success.
* Compiler-known operator facts should be considered as exact/static inputs or
  a separate executor baseline; they must not be falsely credited to learned
  Cell semantics. No such shortcut was silently added here.

The next useful objective is complete-candidate verification/fallback cost,
with consumer multiplicity and unsupported vocabulary accounted for. These
experiments do not yet implement that objective or a production CPU fallback.

## GPU execution and cost

The shared worker accepts three structural packet forms:

* `SPINE001`: 256-wide structural token coordinates.
* `SPINE002`: the same coordinates followed by their elementwise square.
* `SPINE003`: separate local and contextual token bags, each 256-wide.

All use FP16 coordinates/weights and FP32 matrix accumulation. Squaring and
two-block coordinate construction run on the GPU. The tested quadratic,
recursive and declaration-masked packets all match CPU reference scores with
zero 0.5-threshold disagreements. Numerical agreement does not prove logical
candidate correctness.

Representative single runs (one warmup) on M3 Max:

| Standalone operator | Device time | Packed model/routing | Temporary coordinates |
|---|---:|---:|---:|
| Quadratic | 0.500 ms | 91,828 B | 339,968 B |
| Pooled recursive | 0.367 ms | 59,108 B | 1,072,640 B |
| Declaration-masked two-block | 0.940 ms | 100,024 B | 2,140,160 B |

They evaluate different numbers of signatures and outputs, so these are not
acceleration ratios. Graph feature preparation, original-key routing, CPU
verification/fallback and RHS effects are excluded. Context preparation still
runs on the CPU. The fixed-point/base-feature phase was about 2.2 s; coordinate
encoding added roughly 2 s for local tags or 5 s for pooled recursion in Python.

The worker reports scheduled tile FLOPs and nonzero-coefficient FLOPs separately
from unpadded matrix FLOPs; padding and static zero masks must not be presented
as useful reasoning work. This remains Apple matrix execution, not evidence of
NVIDIA Tensor Core utilization or 50% of neural-network throughput.

The original `SPINE001` encoder body is retained as a separate fast path; the
new square/two-block programs use an extended encoder. Final checks cover all
three structural packet versions, plus compatibility with the older opaque-key
packet. Scores and threshold decisions agree with their CPU references.

## Reproduce

Use the same `SNAPSHOT`, `PROFILE`, `SOURCE`, `ANALYZE_QUEUE`, `NATIVE_REPORT`
variables as in the main README. Native full-query controls are reused by hash
and complete answer digest. No native inference rerun is required.

```sh
python -m unittest discover -s experiments/structural_key_memory -p 'test_*.py' -v
python experiments/structural_key_memory/audit.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --source "$SOURCE" --analyze-queue "$ANALYZE_QUEUE" --output out/key-audit.json
python experiments/structural_key_memory/quadratic.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --source "$SOURCE" --analyze-queue "$ANALYZE_QUEUE" --native-report "$NATIVE_REPORT" --output out/key-quadratic
python experiments/structural_key_memory/context_study.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --source "$SOURCE" --analyze-queue "$ANALYZE_QUEUE" --native-report "$NATIVE_REPORT" --output out/key-context
python experiments/structural_key_memory/context_study.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --source "$SOURCE" --analyze-queue "$ANALYZE_QUEUE" --native-report "$NATIVE_REPORT" --typed-mask-only --output out/key-masked
```

Compile the existing `superposed_key_memory/gpu.swift` worker as before. Pass a
generated `input.bin`, its `reference.bin`, and an output report path. Native
egglog, the website and the original default predictor are unchanged.
