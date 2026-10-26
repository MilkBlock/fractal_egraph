# GPU execution contexts and visibility

These are optional execution paths in the actual GPU-owned backend, not the old
CPU-submit assisted matcher or a standalone Cell simulation. They preserve the
original Run publication boundary. They do not yet replace all table storage,
load automatic RipSat certificates, or retain query continuations across Runs.

## Exact parallel RHS class

`--parallel-rhs` compiles an entire ruleset to parallel union effects only when
every action is a copy, Boolean calculation, or union of already-bound values.
There must be no constructor allocation, function access/update, subsume, panic
or fallible arithmetic. Unions must be deferred and parallel rebuild enabled.
The two-union maximum per binding follows the existing union-queue capacity;
larger programs keep the ordered executor rather than losing effects.

Each binding evaluates its own program. Atomic minimum-root linking converges
to the same equality partition. No other action or query observes intermediate
links. A separate completion dispatch publishes watermarks and requests rebuild.
On raytrace the admitted sets are `apply-drop-unions`, `apply-subst-unions`, and
`terms-helpers-helpers`. Most allocating RHS programs still use ordered commit;
this does not claim to fix the entire profiled serial RHS hotspot.

## Rebuild without one host wait per callback

`--rebuild-batch N` repeats the currently selected canonical-rebuild or suspended
callback branch in one encoder with explicit buffer barriers. GPU-written
indirect arguments turn inactive iterations into zero work. A change to the
other branch stops active work until the next batch; stale duplicate groups are
never reused. N is submission granularity, not a saturation limit. Full canonical
passes and final compaction retain their original order. The default is one.

The first version queued both rebuild branches at every iteration and regressed
despite fewer waits. It is retained in the results ledger; the implementation
now queues only the branch active at the start of each batch.

`--merge-target-index` uses the complete typed constructor vocabulary of custom
merge expressions. A callback-only index omits unrelated tables. It remains
valid across unions outside its key sorts; unions on key sorts invalidate its
generation. Congruence unions update the same generation. Container-valued keys
conservatively depend on every equality sort. Final compaction invalidates this
index before ordinary matching/action access. Restored checkpoints reconstruct
key-sort flags from their typed rows; no old generation is blindly restored.

## Versioned query matrices

`--split-matches` cuts eligible queries after the second branching atom and all
subsequent grounded filters, before a later branching atom. This is an explicit
experimental layout, not an optimal layout selection algorithm.

A context row contains:

```
continuation ID | rule rank | delta variant | seed timestamp | seed row
prefix ordinal | maximum witnessed timestamp | epoch | correlated bound slots
```

The prefix runs against the same frozen state as its suffix. The suffix validates
the epoch, restores the correlated slots, and continues the original constraints.
First-new-atom pivot checks stay in their original portions. Sorting by prefix
then suffix ordinal preserves the original ordered RHS population. Contexts are
scratch query work, never visible facts and never independent column marginals.

Only actual prefixes occupy the queue; its capacity is checked. Overflow stops
prefix work and restarts the entire untouched query phase with the original
matcher, including unsplit rules. No RHS has executed at that point. Thus the
capacity affects performance, not completeness. It is not a candidate budget.

## Ridge on the same GPU context

`--context-model MODEL --split-matches` loads a small ridge head per continuation.
Coordinates are constants, typed integer sign/zero/magnitude, Boolean values,
and equality between same-sort slots. Opaque IDs are never treated as numerical
features. A layout fingerprint binds weights to their coordinate program.

GPU evaluation and both exact continuation passes use the same resident context.
Predicted positives run first; all negatives run second. Original binding order
is reconstructed before RHS execution, so score order cannot change merge order.
This implementation evaluates short FP32 dot products, not Tensor Core GEMM.
It does not claim a useful model, reduced exact work, or prediction speedup.

`--context-training` explicitly exports coordinate/label frames for offline
ridge fitting. Labels come from actual successful suffix matches, not a final
graph heuristic. This diagnostic mode is excluded from inference timing. Normal
execution exports no intermediate bindings/features and does no online solve.

```
python3 experiments/device_backend/run.py --source INPUT --build BUILD \
  --output out/context-training --split-matches --context-training
python-with-numpy experiments/device_backend/train_context_ridge.py \
  --run out/context-training --output out/context-model.json
python3 experiments/device_backend/run.py --source INPUT --build BUILD \
  --output out/context-predicted --split-matches --context-model out/context-model.json
```

## Verification

`test_backend.py` checks ordinary native/GPU execution. `test_context_execution.py`
adds correlated multi-witness joins, timestamps, later function changes, union,
subsume, typed merge-index invalidation, queue overflow, all-positive/all-negative
models, and rejection of stale model layouts. New paths are checked after each
Run against native and against the original device path. Parent forest shape may
differ under atomic unions; canonical partitions and declared rows must agree.

The full raytrace comparison remains device-to-device until the previously known
native endpoint mismatch is resolved. Equal step counts alone do not pass it.
See `context-execution-results.json` for measured variants and their gates.

## Recorded full-run result

The final combined variant took **42.874 s**, versus the reused **43.383 s**
control (one sample each). GPU device time increased from **33.686 s to 34.511 s**;
the small wall-time decrease is not evidence of a substantial acceleration.
The full 10,097-Run schedule, all declared rows/values, visibility and equality
partitions agree with the saved device control: 699,784 rows and 226,652 unions.
Atomic union trees need not have identical parent-pointer shape.

The first eager-prefix/all-branches-batched attempt took 44.680 s. Its 528,770,350
prefix attempts fell to 53,778,710 in the final layout; 21 overflow Runs still
used complete original-query restarts. These are context attempts, not avoided
native candidate rows. Rebuild waits fell from 5,445 to 1,704, but the final
rebuild GPU time was 6.772 s, above the 5.623 s control. Restricting invalidation
by merge key sorts removed only 25 iterations on this workload.

All new flags remain opt-in. The 60 ordinary execution checks and 18 context
cases passed. A fitted small ridge model also passed native checking; it was
calibrated on four contexts, not a learned Pass-3 model or an accuracy claim.
This work supplies correctness-tested execution alternatives and records a
negative performance result for the intended large speedup.
