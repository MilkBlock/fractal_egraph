# Source-only pipeline: recovered Pass-3 performance and correctness

Implementation: `a5107f0`.

The execution contract remains:

```
source.egg → source_sk.egg → source_sk_mk.egg → runtime
```

The key stage reads source declarations and consumers. The marker stage reads
only that SK program, measures its native plan alternatives and ordinary GPU
work, selects layouts, and seals checked closure programs into source comments.
The runtime reads the final file. No previous Analyze catalog, historical layout,
hardcoded loop choice, external certificate file or implicit key policy is loaded.
The worker and solver are compiler/backend configuration, not additional semantic
inputs. Historical performance records are validation controls, never planner input.

## Final measurement

Actual complete saved **Raytrace Pass 3**, one execution per lane, same final
binary and frozen Metal worker. These numbers exclude offline compilation, setup,
graph export and extraction, as did the old 12.34-second baseline. Native effects,
merge/rebuild, event handling, Cell maintenance and GPU handoff are included.

| Lane | Inference |
|---|---:|
| Current same-policy GPU, no Cell declaration | 16.502049 s |
| Automatically generated final source | **12.132912 s** |
| Historical acceptance threshold (not a causal denominator) | 12.340255 s |

The current paired reduction is **26.48%**. The final run is below the historical
threshold, but the old stable-choice policy differs from today's unmodified
input policy; do not present that historical comparison as an isolated speedup.
Native setup was 0.157871 s and GPU/Cell setup 2.743249 s. Do not attribute the
earlier 30-second cold shader setup entirely to these changes.

An intermediate build measured 11.662845 s. The final build adds the early
self-contained-input check and measured 12.132912 s; the latter is reported,
not the fastest run. There is no averaging or cross-workload performance claim.

## Independent correctness gates

* Timing pair: exact original rows, function values, equivalence and visibility.
* Independent CPU reference has **Cell assistance disabled**. It is the native
  sorted-submission reference, not another execution of the same closure shortcut.
* Final-build GPU audit checks **478,137 incremental queries**, including empty
  answers, against independent native queries at their actual frontiers.
* CPU vs audited GPU: **699,068 original rows / 524,631 mapped e-classes** agree.
  Only the eleven additional proof-gate tables are projected away.
* Audit and uninstrumented timing snapshots are byte-identical:
  `8e88a2d2de223e6e8e4f0af49897f19d8762fb129d3f34b224deb299f552d95b`.
* Production Tiger selects identical DAGs for **29 functions** (8,006 selected
  rows/e-classes). This is actual extraction, not a comparison of cost estimates.
* 29 source/compiler/runtime integration tests and 69 bench protocol tests pass.
  They include renamed closure roles, isolated final-file loading, external
  updates/union, finite Run boundaries, tampered wiring, source/trace checkpoint
  mismatches, ordered merges and partially rewritten ruleset order.

No pristine/fused-native equivalence claim is added. The CPU reference inference
path was unchanged by the later early-input-validation refactor and was reused;
its binary identity remains in the verification record. GPU timing and audit
both use the final binary. Source, runner, worker and graph hashes are recorded
in `results.json` and the underlying skill reports.

## Repairs

### Restore proofs as explicit source declarations

`cell-runtime/v1.closures` carries source-derived selection roles, scalar
descriptors and complete typed port programs. The loader checks the current
helper universe, guards, producer effects and wiring. It validates rather than
discovering a replacement plan or reading an old sidecar. Ordinary Run keeps its
frontier; closure operations remain confined to checked Saturate boundaries.

This input produces **31 closure programs**, including the structurally recognized
candidate/selection scheme and 17 scalar rules. Closure-owned consumers are
excluded from duplicate marker takeover. Runtime evidence includes 12 joint
boundaries and 601 native comparison-closure batches. Fresh-DAG macro instances
remain **zero**; this is not twelve free whole-Cell replays.

### Let the solver retain the original executor

Previously runtime-priced selection forced each observed query to pick a Cell
layout. It also treated uncached private subscriptions as having no update cost.
The model now includes ordinary GPU starts/probes/row visits/output work as a
legal alternative and charges the recorded committed updates to watched tables.
It chooses **224 complete-query plans** in addition to the 31 closure programs.
The objective is a measured-work estimate, not a proof of minimum elapsed time.

The familiar loop-context program is selected from source and measurements:
84 live paths, 2,814 reused reads and 956 resumed evaluations. Those numbers
are observed results, not constants in the compiler or runtime.

### Respect incremental frontiers when using markers

Physical source rewriting had moved changed rules to the end of insertion-ordered
rulesets. The emitter now preserves the entire affected ruleset's source order,
retains source binding names, avoids redundant self-publication, and does not
rewrite an existing table merely to rename it as a marker.

That alone did **not** repair the Pass-3 graph: a materialized view can expose an
old match at a new incremental frontier. On this input, `Smaller` still lost one
original row, including with Cell query takeover disabled. A source containing
non-Unit `merge old/new` therefore retains its original consumers and uses the
explicitly declared runtime prefix markers with their original query watermarks.
This is a conservative semantic gate, not a rule-name list or search budget.

Be precise about this result:

* **Zero new physical marker tables and zero textual LHS rewrites on this input.**
* **140 runtime shared-prefix markers**, described by the selected plans in the
  final file's comments. Their maintenance and use are included in the timing.
* Safe source-marker fixtures still exercise real tables, RHS publication and
  consumer rewriting. A general physical rewrite proof for arbitrary ordered
  merges is still absent; this report does not claim otherwise.
* This unannotated input has no structural-key consumer, so the SK pass adds
  zero keys. The original selection policy is preserved, not silently replaced.

## Reproduction

The versioned bench skill is the entrypoint. Fresh generation uses:

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py structural-pipeline \
  --source INPUT.egg --output out/new-pipeline --worker MATRIX_WORKER
```

All expensive profiling is of that preceding SK file. To reuse the **complete,
byte-identical-input** checkpoint from this experiment without collecting it again:

```sh
python -m tools.cell_spine.source_pipeline \
  --source out/key-demand-pass3/plain-exact/source_sk.egg \
  --output out/source-triple-pass3/mk --resume --gpu-worker MATRIX_WORKER
```

This writes a new `build-N`, preserving earlier files and failures. Regeneration
reproduced exactly the measured final source SHA:
`62176a5e03fd69e1a56cea67b14c879f67edce738ae90060d63b857460955c6f`.
The compressed trace and manifests are compilation caches, not runtime inputs.

The latest final file is
`out/source-triple-pass3/mk/build-4/source_sk_mk.egg`. Execute it without any
layout/library arguments:

```sh
target/analyzer/release/cell-assisted-egglog --mode program --backend gpu \
  --source out/source-triple-pass3/mk/build-4/source_sk_mk.egg \
  --gpu-worker MATRIX_WORKER --output out/new-execution
```

Full verification is automated by the skill:

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py program-modes \
  --backend cpu --source SK.egg --cell-assistance off --export-extraction \
  --output out/cpu-check --no-build
python3 skills/cell-assisted-egglog-bench/scripts/bench.py program-modes \
  --backend gpu --source SK_MK.egg --worker MATRIX_WORKER --query-audit \
  --export-extraction --output out/gpu-check --no-build
python3 skills/cell-assisted-egglog-bench/scripts/bench.py program-verify \
  --reference out/cpu-check/original-cpu --candidate out/gpu-check/original-gpu \
  --output out/quality-check
```

Do not report audit or search-counter timing as a speedup. Saved evidence:
`out/source-repair/final-timing`, `final-build-audit`, `cpu-reference-export` and
`final-build-verification`. Prior failed materialized-source variants remain
under `out/source-repair`; they were not silently relabeled as successful runs.
