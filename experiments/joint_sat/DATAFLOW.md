# Complete terms closure via parameterized Cell ports

This extends the actual shared runtime's explicit Saturate hook. The original
fresh, single-producer DAG macro remains the cheapest case. If its preconditions
fail, the same source-compiled port program now executes the complete three-set
schedule, including multiple producers, missing inputs, mutable ExtractedExpr
rows and union. Each original Run keeps its frozen input and native RHS /
merge / rebuild wave. Queries follow fixed constructor/function ports; they do
not rerun the original relational LHS or enumerate concrete closed-cell states.

This path is currently **CPU Cell dataflow**, not GPU matrix execution. On the
full saved raytrace Pass-3 workload all 12 terms boundaries use it; the GPU
control still executes its complete incremental joins. Zero GPU dispatches in
the assisted lane is intentional and must not be advertised as a GPU throughput
improvement. The native engine still owns all explicit facts and effects.

## Scoped negative certificates

The six checked helper rules implement initialization, cost improvement and
Smaller resolution. If every TCPair equivalence class has one immutable cost,
a resolved ExtractedExpr cost can only decrease inside this closed schedule.
For one complete candidate with cost c, failure of `c < current_cost` therefore
proves that subsequent decreases cannot enable it. Remove that candidate's
internal subscriptions for the rest of the scope. New candidates still enter
normally, and all retired candidates are requeued on leaving the scope.

The proof checks the installed producer universe of TermAndCost (TCPair,
Smaller and ExtractedExpr), visibility, current TCPair costs **and pending
Smaller equations**. The latter matters: an external union may equate a cost-3
TCPair with an unresolved `Smaller(cost-8,cost-10)`. Today's uniform TCPair rows
alone would miss the future conflict. Propagate existing known minimum costs
through the pending graph and reject inconsistent equations. Unseeded cycles
cannot create a TCPair using these helpers. Failed proof retains complete port
queries; no threshold or sampled candidate selection is involved.

The scope cannot contain external updates. After handback, deletion, new
candidates and new cost aliases receive full checks again. Regression tests
exercise these cases, an inconsistent pending Smaller, ordinary Run boundaries,
native failure behavior, and CPU/GPU shared-runtime integration. Tests of this
optimization use the same sorted native submission protocol: fused native can
produce a different orientation of intermediate Smaller records even with the
optimization disabled.

## Index representation

Hot keyed and output-directed reads use exact hash indexes. Binding and read
lists use inline buffers with ordinary heap spill for wider rules, not a slot
limit. Dependency watchers only receive changes from relevant tables. Closure
construction and emitted batches retain deterministic ordering; changing a map
implementation does not change native action order.

## Complete Pass-3 measurements (one run per lane)

| Variant | GPU without joint | Assisted | Strict public graph |
|---|---:|---:|---|
| Initial port dataflow | 18.337 s | 20.753 s | pass |
| Compact exact indexes | 18.575 s | 17.382 s | pass |
| Scoped dominated-candidate retirement | 18.212 s | 15.249 s | pass |

The last run is ~1.194x versus GPU without sat-skip, **not yet the requested large
speedup**. It is ~1.616x versus the same-submit CPU (24.647 s). The fused native
run is 24.073 s but retains its previously documented different intermediate
helper records; it is not the exact-graph denominator. Timings exclude setup,
graph export, diagnostics and extraction. This is the complete saved Pass-3
schedule in the local fork, not pristine upstream or a fresh three-pass eggcc
compilation. Scalar numeric closures described in `../scalar_cells/README.md`
are enabled in these assisted runs too.

For the improvement rule, seed reevaluations fell from 2,034,593 to 202,085;
73,608 failed seeds received the scoped certificate. Total program keyed reads
fell from 8,288,272 to 2,790,748. Final inference retained exactly the original
public rows, values, equivalence and visibility against both same-submit CPU and
GPU-off controls. Fresh-DAG macro instances remain zero; the 12 completed
boundaries are dataflow executions, not 12 reused concrete closures.

Reproduce with `skills/cell-assisted-egglog-bench/scripts/bench.py run
--joint-sat --output out/FRESH`. Saved reports: `out/raytrace-joint-dataflow-v1`,
`out/raytrace-joint-compact-v1`, `out/raytrace-joint-retirement-v1`.

Production Tiger also passed for the retirement run: the three extracted
programs are byte-identical, with 29 functions and 8,018 compared public rows.
SHA256: `c48fe2b212d341b1c3f3de3ef94e77e3252b661e68be3bf60fbd417bb5ad3ce9`.
