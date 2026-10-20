# Certified comparison saturation inside the joint Cell

This improves the existing terms Cell handler in the actual saved raytrace
Pass-3 runtime. It does not add a catalog of concrete Cells, change an ordinary
`Run`, or bypass native effects.

## What is skipped

The checked `terms-helpers-helpers` contract contains three rules with the same
structural input: `Smaller(TCPair(t1,c1), TCPair(t2,c2))`. Their guards select the
left argument for `c1 <= c2`, otherwise the right argument. All three RHSs only
union the Smaller result with that argument.

The existing joint proof establishes that every visible TCPair alias in a
resolved class has the same cost, and that this remains true in this scope.
Thus the closure can compare the costs once without enumerating Term aliases.
It propagates a derived cost through pending Smaller dependencies, then submits
the derived equalities to native egglog in a batch. Native union and rebuild
still publish all actual changes. New rebuild events are consumed before the
closure is declared complete.

The runtime keeps subscriptions for missing reads and for all three original
comparison kernels. A later ordinary Run, deletion, update or conflicting alias
therefore still has a complete frontier.

## Conditions and boundaries

- The existing helper-rule and schedule contract must match exactly.
- The cost universe must be closed, current/pending cost aliases consistent,
  and TCPair/Smaller witnesses visible.
- Foreign functions keyed by TermAndCost or the PotentialExtractedExpr marker
  cannot observe a reordered merge. The check also follows fixed container
  element types; function/custom containers are conservatively unsupported.
- Unrelated containers such as `Set Expr` remain allowed.
- Only the inner comparison **Saturate** is replaced. Producer and helper Run
  waves, checked arithmetic, errors and outer scheduling remain native.
- Unsupported states fall back to the existing port program. Unknown cyclic
  costs are not guessed. No search budget or iteration cap is introduced.

This is a specialized certificate for the installed terms helper contract,
not a generic proof for arbitrary effectful rulesets.

## Measurement

Each configuration ran once. Native/CPU/GPU-off controls were reused from
`out/pass3-compact-layout`. The final on/off pair has identical binary and worker
hashes. Profiling was disabled; earlier profiling edits were left unchanged.
Build, initialization, graph comparison and Tiger are outside inference timing.

| Latest same-binary ablation | Closure off | Closure on |
|---|---:|---:|
| Complete inference | 18.634 s | **17.708 s** |
| Joint saturation handler | 3.329 s | **2.074 s** |
| Device | 5.780 s | 5.885 s |
| Ordinary dataflow Runs | 3,660 | **1,639** |
| Original comparison bindings emitted | 73,371 | **0** |
| Keyed reads, including new cost lookups | 3,959,289 | **3,301,179** |
| Row notifications | 2,458,173 | **2,425,040** |
| Native union notifications | 239,476 | **239,476** |

Inference improved **5.0%**, and the joint saturation pool improved **37.7%**.
The GPU did not get faster in this ablation; the improvement belongs to the
Cell closure path.

The new path handled 1,185 inner saturation intervals, 584 of which needed no
native changes. It generated 89,226 equality proposals and committed them in
601 native batches. Proposals are not new facts or a count of effective unions.
Its planning took 0.223 s and native effect submission/rebuild took 0.831 s;
these are included in the handler time, not additive savings.

The 12 outer joint boundaries still use dataflow for producers and helpers.
Fresh-DAG macro instances remain **zero**. Ordinary Cell query coverage remains
30,566 frontiers / 1,918,606 bindings. The improvement is the newly certified
inner fixed point, not increased Cell coverage.

## Validation

- Complete same-commit CPU/GPU facts, values, equivalence and visibility pass.
- Complete Cell-on/off graph comparison passes.
- Production Tiger selected-DAG comparison against same-commit CPU passes.
- Full independent native-frontier audit passes: 478,136 queries. It checks the
  remaining queries; replaced closure internals are covered by graph checks
  and the targeted native regressions.
- Regressions cover deep chains, pending children, finite Run handback, missing
  inputs, destructive updates, hidden witnesses, conflicting cost aliases,
  foreign merge callbacks, container keys, and unrelated container declarations.
- The pre-existing fused-native graph/Tiger difference remains. No pristine
  equivalence or pristine speedup is claimed.

An earlier same-binary prototype pair measured 21.059 → 19.339 s. A subsequent
guard attempt incorrectly rejected the wrapper around `Set Expr`; its run
`out/sat-comparison-final` had **no active comparison closure** and is excluded
from optimization claims. The final guard and regression fix use the container
value type. All runs are retained in [sat-comparison-results.json](sat-comparison-results.json);
the latest pair above is the result, not an average or the fastest chosen run.

## Reproduction and code

Use the `cell-assisted-egglog-bench` skill's `run --all-rules --joint-sat`
workflow. The closure is enabled by default. Set
`CELL_COMPARISON_CLOSURE=0` for the same-binary ablation.
Inspect `gpu.saturation_diagnostics.joint.comparison_closure`: `null` means the
schema is unsupported; zero counters mean it was not used.

Implementation: [comparison.rs](../../cell-assisted-egglog/src/parameterized_cell/comparison.rs),
with scope activation in [joint.rs](../../cell-assisted-egglog/src/parameterized_cell/joint.rs)
and native regressions in [joint_sat.rs](../../tests/joint_sat.rs).
