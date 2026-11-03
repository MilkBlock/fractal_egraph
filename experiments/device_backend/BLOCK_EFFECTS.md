# Deferred block effects in the certified terms program

This is part of the actual experimental GPU-owned backend, not a standalone
Cell simulation. It applies to the certified `terms`, `terms-helpers`,
`terms-helpers-helpers` nested Saturate program. Finite Run and unsupported
rule environments keep their existing behavior.

With a stage Cell, block effects are now enabled by default. `--block-merge`
also enables the certified joint path without stage snapshots;
`--eager-stage-merge` is the explicit same-program ablation. Existing Bake files
remain loadable: their complete rule/merge/read footprints are still checked.

## What crosses the stage boundary

```text
terms: produce candidate nodes in the private working graph
  ↓
helpers: produce ordered (key, value, owner) set requests
  ↓
prepare merge expressions using a per-owner pending value
  - retain every required Smaller / candidate node
  - do not update ExtractedExpr rows
  ↓
comparison: read those local expressions and compute a private closure
  - function requests are still pending
  - publish a compact basis of new equality edges
  ↓
GPU block merger
  - union the new edges with the CURRENT equivalence relation
  - publish one prepared function value per affected key
  - refresh affected local rows and ports
  ↓
continue the existing nested Saturate until its fixed point
```

A block here batches many root contexts. It is not a new full egraph per root.
The global graph outside the transactional joint invocation is published once
at the joint exit. Local constructors are still materialized in its private
working row store; this is not a table-free matrix engine.

The helper rule contract proves that set is terminal in each RHS and that the
comparison rules do not read ExtractedExpr. Consequently the function update
can cross the comparison stage without hiding an input it needs. Subsequent
helper/terms iterations run after block merge and see the resulting values.
The pending context occupies three existing per-owner slots: presence, prepared
value, and original function-row identity. It is versioned with the stage view.

## Correctness boundaries

- Ordered requests are preserved, including repetitions. The custom
  `Smaller(old,new)` merge is evaluated in that order in the local context;
  it is not assumed associative, commutative or idempotent. Intermediate merge
  nodes remain available to comparison. Only function-table publication is
  combined per key. An unexpected conflicting function-row write rejects the
  transactional fast path rather than silently overwriting another writer.
- An equality block contains one edge for each old UF root that became a
  non-root. Its size is checked against the number of successful local unions.
  Owned compact buffers retain these edges independently of reusable scratch.
- The block merger computes the join of its edges and live equality, using
  atomic unions. It NEVER overwrites live parents with the old local UF.
  Therefore a newer union from another block survives. Replaying an equality
  block adds no duplicate unions. Blocks cannot cross invocation identity domains.
- Function values are canonicalized after equality merge. Publication preserves
  producer timestamps unless the new union changes the value, in which case
  it uses the comparison timestamp. New function rows enter the next matching
  context. Finite Run was not changed into Saturate.
- Overflow/panic rejects the private transaction without changing the published
  graph. Audits check that helper producers and merge-expression preparation
  leave function rows untouched; normal stage patch audits remain available.

## What synchronization was removed

The comparison program returns a pending edge block. Its merger, function
publication and affected-row maintenance are encoded in one following command
buffer, without a separate host wait for union publication. CPU waits for
matching capacity/control and comparison convergence still exist. This does not
claim arbitrary stages can run concurrently, or that all internal matches have
been replaced by a Bake-generated effect program.

## Reproduce

```sh
python3 experiments/device_backend/test_block_effects.py \
  --build out/device-build-block-cross-stage --output out/new-block-tests

python3 experiments/device_backend/run.py --source /path/to/pass-3-flat.egg \
  --build out/device-build-block-cross-stage --output out/new-block-run \
  --stage-cell out/multi-stage-native-domains/terms.cell.json \
  --matrix-saturate --check-every 0 --save-state \
  --capacity 1048576 --match-capacity 1048576
# Add --eager-stage-merge for the same-binary control.
```

`test_block_effects.py` checks deferred effects against eager execution and
native egglog: merge-unlocked parent computation, shared inputs, repeated custom
merges, external union feedback, finite Run and rollback. Its Swift probe uses
the real GPU merger with synthetic edge blocks in both orders, including a
pre-existing union absent from both blocks and duplicate replay. That probe is
an algebra test, not an eggcc performance result.

`block_effects` counters in normal reports distinguish requests, prepared
function-key publications, committed union changes and removed publication
waits. Requests and key publications are not counts of actual table mutations.

## Recorded result, 2026-10-28

[Results](block-effects-results.json) and [checks](block-effects-checks.json)
record the same saved full Raytrace Pass-3 input, same worker binary, no ridge,
and stage contexts enabled. Single clean runs:

| Publication | Complete inference |
|---|---:|
| `--eager-stage-merge` | 44.003 s |
| Deferred block effects | 41.814 s |

The observed reduction is 4.97%; no stable speedup or occupancy improvement is
claimed from one pair. The earlier different-binary historical run was 39.798 s;
against that run the new joint invocation time was 10.398 s vs 10.349 s, while
most whole-run difference was outside the joint program. Both comparisons are
retained rather than presenting an intermediate best time as the final result.

All 12 joint scopes were accepted. Across 2,826 phases, 113,916 set requests
remained pending through comparison and were consumed at block merge; there
were 61,074 per-key publication entries (not necessarily changed table rows),
90,045 committed union changes and 1,186 removed standalone publication waits.

75 checks passed: 8 block-effect checks, 14 stage-context checks, 19 comparison
checks and 34 joint-program checks. The full audit checks every stage patch and
113,916 producer requests for premature function/union publication. Complete
facts, primitive values, visibility and equivalence agree with the eager GPU
control at 699,784 rows / 524,742 eclasses. Original outer scheduling and internal
Run ordering agree; physical function publication is intentionally later.
Independent native comparison of the real initial terms scope also passes at
27,793 rows / 21,655 eclasses. The backend's pre-existing full-Pass difference
from native egglog remains outside this change.
