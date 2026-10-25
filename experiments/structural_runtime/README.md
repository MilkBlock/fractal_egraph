# Live Pass-3 spine proposals with exact CPU completion

The current shared-condition/event-coordinate revision is described in
[SHARED_RESULTS.md](SHARED_RESULTS.md). RESULTS.md retains the original measurements.
The direct commit-to-bitmap backend (`--coordinates markers`) is described in
[MARKER_RESULTS.md](MARKER_RESULTS.md). It compiles the same feature vocabulary
into counted differential views; it does not enable the separate `.egg` marker pipeline.

The solver's joint spine summaries can now be imported as **new** predictive
features with `--summary-markers` at export time. This is separate from the
same-vocabulary bitmap representation. See [SUMMARY_FEATURES.md](SUMMARY_FEATURES.md)
for exact contracts, target masking, paired live accuracy and the measured limits.

This is an opt-in query provider in the **actual native execution loop**, using
`cell-assisted-egglog::table_live::SaturationHook::query_cells`. It replaces the
search of selected rules, not their RHS, merge, rebuild, Run boundaries, or
saturation decisions. It is not enabled by default, a closure certificate,
sat-skip, or a new pristine-egglog implementation.

The exported predictor is the previous `structural_key_memory` model. Its
vocabulary comes from Analyze constructor spines, not a parser-written purity
theory. The model was trained on the saved final graph of this same program;
this experiment tests **live maintenance and exhaustive completion**, not
unseen-program generalization. Scores are not probabilities or proofs.

## The exact CPU complement

The exporter selects ordinary-constructor anchors whose row fixes **every
variable shared between LHS atoms**. Remaining atoms have independent local
witnesses once that anchor binding is fixed. This is checked again against the
native compiled rule when loading the library. No primitive guard or residual
is erased, and unsupported queries keep their ordinary matcher.

For example, after `Bop(op,x,y)` binds `op,x,y`, its three purity conditions
can be checked independently. For a more general supported atom with local
witnesses, the index preserves whole correlated rows, not column marginals.

1. Native committed row changes update exact projected-key indices.
2. Reverse subscriptions map changed predicate keys to affected anchor rows.
   A constructor change also exposes the anchor itself. This step does not
   precompute the answers that the GPU is supposed to predict.
3. Consumers reference shared `(projection, key, version)` condition blocks.
   Valid exact answers, including empty ones, are reused directly; unknown blocks
   receive GPU scores. Every anchor remains covered.
4. CPU stops only when an exact current projection is empty. Otherwise it
   retrieves all witness sets and constructs their complete binding product.
5. At least one witness must meet the **original rule's native timestamp
   watermark**. The existing runtime sorts the returned complete bindings and
   submits all rules' original RHS at their original Run boundary.

Thus even an adversarial predictor rejecting every anchor cannot lose a
binding. This is not “GPU proposals plus a second full native Join”: it is an
exhaustive factorized query whose order the model can improve. It also does
**not** imply arbitrary LHS queries admit this factorization. The shared anchor
condition is a significant current restriction.

Let `A_delta` be the affected anchors, `d` the residual atoms, `W` their
witness rows, and `B` the emitted complete bindings. The matching work is
`O(A_delta*d + W + B*slot_count)` in the worst case, plus commit-index and
coordinate maintenance. Early emptiness can reduce the `d` factor; approximate
negative scores by themselves cannot. The output product can still be large.
Report the exact-only CPU lane to separate this factorization from GPU credit.

## Incremental features and the persistent GPU service

Only the 17 constructor names read by the 94-token feature program are
subscribed, with **all** current rows and alternatives retained. Unrelated
container-valued constructors are not part of this program. Initial state is
bootstrapped once; later updates use native row journals, including rebuild,
function replacement, disappearance and subsumption. Native rebuild supplies
changed canonical rows after union; it no longer clears every coordinate.

Local row/port changes have a direct publisher. Cross-node subscriptions name
exactly the head or row family inspected by the two-edge program, including
currently absent heads. Events coalesce dirty coordinates; the consumer boundary
publishes only coordinates needed by unknown condition blocks. Dormant consumers
do not trigger eager recomputation. Compact typed IDs and row slots replace
string/whole-key adjacency references. No per-Run whole-graph export or Python
feature extraction occurs. Publication inside a query is charged to query time;
reports show maintenance plus query cost so moving work cannot hide it.

Equal structural token signatures share their **prediction**, not their exact
truth, binding, or eclass. The immutable model's scores remain valid as
predictions of that signature even while actual predicate membership changes.
Exact CPU indices are updated independently. Complete immutable witness vectors
are shared by reference, retaining each consumer's slot map. Native timestamp
changes invalidate payload versions even when the structure is unchanged.

The Swift worker stays alive. It loads FP16 weights once and accepts binary
requests containing only new token signatures. It uses the existing signed
coordinate encoder and grouped `simdgroup_multiply_accumulate` kernel, returning
FP32 scores. Rust caches predictions of identical signatures. This is Apple
Metal matrix arithmetic, not a measured NVIDIA Tensor Core result. Small
request batches can still be dominated by dispatch/transport costs.

## Integration and verification

This checkout predates the assisted-runtime crate. `build.py --runtime-repo`
links to the existing checkout without editing it and reuses the bench skill's
native graph-comparison implementation. Generated Cargo files, kernels and
binaries remain in `out`. Build provenance records the dependency checkout,
dirty state, source hashes and actual binary hashes.

`run.py` runs one sample per changed lane, preserves failures, and reuses
unchanged results by identity. Its modes are:

* `native`: original native queries, existing sorted commit protocol.
* `exact`: exhaustive CPU factorization, fixed condition order, no GPU model or
  structural feature maintenance.
* `audit`: GPU plus exact completion, independently compared against native
  **full incremental bindings at every claimed Run**. Not a timing baseline.
* `gpu`: the same GPU/completion path without the independent audit query.

Without `--query-worker`, all other rules remain on CPU. With it, they use the
existing full GPU matcher. The additional `baseline` lane runs that matcher
without this provider. No joint sat-skip library is installed in this experiment.
Times must not be compared directly with historical GPU/Cell configurations.
`--extract` reuses the bench skill's production Tiger and DAG quality checker.
Its three legacy lane labels are mapped to native/exact/gpu here; `native` does
not mean the pristine import. Full graph comparison includes facts, function
values, equivalence and visibility, without dropping original tables.

```sh
python3 experiments/structural_runtime/build.py --runtime-repo "$RUNTIME_REPO" --output out/structural-runtime-build
# Run export.py once with the existing snapshot/profile/source/Analyze paths.
python3 experiments/structural_runtime/run.py --runtime-repo "$RUNTIME_REPO" --build out/structural-runtime-build --source "$PASS3" --output out/structural-runtime-pass3 --extract
```

For the full GPU comparison, add `--query-worker` to **build.py**, then add
`--query-worker out/structural-runtime-build/query-worker --lanes native,baseline,exact,audit,gpu`
to **run.py**. The predictor worker and existing matcher are separate persistent
services. The predictor does not replace the existing worker's generic matching.

`--coordinates legacy` retains the original feature backend for ablation;
`--coordinates markers` directly updates compact feature bitmaps from commits,
without query-time graph traversal. `events` retains the adjacency-based
publisher as a control. Marker audit uses that independent graph implementation.
`--no-shared-conditions` disables condition sharing. Both enter the run identity.
Audit additionally checks requested coordinates against the uncached definition.
`--model FILE` loads a separate exported library. `export.py --residuals all`
optionally admits constructor/function residuals under the same single-bound-key
contract; the default preserves the original seven-rule scope. This is not
arbitrary-rule support. `--reuse-baseline DIR` reuses an unchanged full-GPU lane
only after checking its source and worker identities and prior strict graph gate.

`--reuse-native OLD_RUN` is an explicit historical-control option only when
changes are confined to this hook/model/worker. Kernel, harness, selection
protocol or schedule changes require a new control. Historical timing is
labelled as such; every new candidate still undergoes strict graph comparison.

Focused regressions exercise all-negative predictions, multi-witness products,
late fact arrival, no-change rounds, slot aliases, union, subsume and function
replacement. `check_gpu.py` tests three consecutive binary requests against the
unchanged CPU FP16 model. See [RESULTS.md](RESULTS.md) for the measured result,
including the regression against the ordinary GPU baseline. `run.py` also emits
per-rule exact-check depth distributions and fixed-rule-weight reach curves.
