# Live Pass-3 spine proposals with exact CPU completion

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
3. GPU scores order the residual conditions, lowest score first. Every anchor
   is checked, including predicted negatives and previously unseen shapes.
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
function replacement, disappearance and subsumption. Union clears feature
memoization; current canonical rows still come from the native event feed.

Incoming-port changes invalidate that child's local features. Changes to a
constructor result invalidate its own and its two ancestor levels' features,
matching the compiled two-edge vocabulary. There is no per-Run whole-graph
JSON export or Python feature extraction.

Equal structural token signatures share their **prediction**, not their exact
truth, binding, or eclass. The immutable model's scores remain valid as
predictions of that signature even while actual predicate membership changes.
Exact CPU indices are updated independently and always consulted.

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
