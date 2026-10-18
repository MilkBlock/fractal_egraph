# Shared, batched, bounded-memory GPU table runtime

This implements the performance priorities from the continuous raytrace result:
(3) reduce synchronous round trips, (2) share table/index storage, (4) stop retaining
old matches and unbounded row histories. Correctness (5) is checked independently
at each query boundary and on the entire final graph.

## 3. Batch one ruleset, not one rule

One Metal worker/device/queue serves all selected rules. Each original `Run`
boundary sends one binary batch for its active queries. The normal update, index
maintenance and query kernels are encoded into one command buffer; only actual
output-capacity overflow retries the uncommitted query work. The previous native
commit acknowledgement is carried by the next request, with no separate ACK RPC.
The host retains exclusive ownership of the native graph during the batch.

All queries read the same pre-RHS snapshot. Returned bindings are sorted per rule,
then the original native actions execute as one group. A failed batch poisons the
session; unconfirmed work is not silently replayed. Union, custom merge and rebuild
still execute in native egglog. This is an indexed integer GPU query engine, not
GEMM/Tensor Core execution and not GPU execution of the original RHS.

## 2. Share data and projections

Each relevant native fact has one row slot across all rules and all occurrences
of its table. Tuple indexes are interned by `(native table, key-column list)`;
variable names and rule identity do not duplicate the index. Query-specific atom
wiring retains literal checks, repeated-variable constraints and exact guards.

Native rows are canonicalized once per committed version. Raw native value IDs
are used for identity coordinates, with an explicit bound mask so ID zero remains
valid. Comparisons occur only between slots whose sorts agree in the resolved
query. Actual i64 payloads remain separate pairs of 32-bit words for exact guards.
There is no per-rule string-to-identity dictionary or CPU construction of every
fragment's complete binding. GPU results directly carry the resolved slot values.

The native row timestamp is now included in the commit feed. A touched journal
key is not automatically a new version. Conversely, delete/reinsert with identical
values is a new version when its native timestamp changes. This distinction is
necessary for stateful custom merges and is regression-tested.

## 4. Remove historical match storage

Native rule watermarks determine which matches are new. Delta ownership uses the
first original atom whose row timestamp reaches that watermark. Successful match
lists are batch-local outputs, not a permanent record of every previous match.
Rows use reusable slots plus generation numbers. Index entries pointing to an old
generation cannot accidentally refer to a new fact occupying the same slot.

The append-only index event log is rebuilt from current rows when its allocation
fills. Geometric allocation leaves room for another live-set-sized wave of
updates. Rebuilding resets event cursors, while native query watermarks suppress
already consumed matches. A delayed ruleset still sees old-but-unconsumed live
facts after compaction. There is no sampling, match budget or approximate eviction.

Memory is bounded by peak live row slots, shared index count and output-buffer
high-water capacity, rather than total historical applications. Allocated buffers
currently retain their high-water capacities; this is not a promise of immediate
RSS shrinkage after a large graph deletion. The native egraph remains explicit.
GPU buffer sizes are reported separately from process RSS.

## 5. Correctness and execution ordering

The optional checker now uses **independent, cached native semi-naive queries**,
including native timestamps. It does not filter a full native query using the
GPU's own event frontier, so a missing GPU event is observable. The checker never
supplies candidates to the unchecked production lane.

The benchmark has three distinct lanes:

1. **Native original:** the original fused search/apply execution and schedule.
2. **CPU sorted-commit:** original native cached queries followed by the same
   sorted, grouped original-RHS submission path used by the GPU.
3. **GPU shared:** shared indexed queries, then that same submission path.

The second lane is a real CPU control, not pristine or the original fused engine.
The GPU must agree exactly with it on final facts, primitive values, visibility
and equality modulo allocated IDs. This is a whole-graph check, not merely equal
cost or node counts. The original fused engine remains a separate reference:
its different ordering can select different terms through the order-sensitive
`Smaller` merge. We continue reporting its strict comparison; we do not rename
that failure into a success or delete Smaller from the primary correctness gate.
Sorted commit is reproducible for a given checkpoint; it is not a mathematical
canonical form invariant under arbitrary input renaming.

A separate stopping-condition bug was fixed: deleting private binding worklists
inside a rule had counted as graph growth even for a no-op RHS. Normal execution
now clears these lists outside the rule's change report, on success or error, using
post-rebuild keys. The interruptible tracing lane retains its earlier per-item
consumption protocol. Ordinary egraph execution is unchanged.

These changes still **do not use baked Cell guarantees to omit internal LHS
atoms**. They improve the complete table-query baseline. Only supported positive
queries/table-bound guards and the original static schema are accepted; schema
changes and unsupported schedules are not guessed.

## Reproduce

From the `codex/cell-assisted-egglog` worktree, using the current live protocol **9** (the recorded measurement used protocol 8):

```sh
mkdir -p out/shared-family
xcrun swiftc -O cell-assisted-egglog/matrix.swift \
  -o out/shared-family/matrix-metal

python3 cell-assisted-egglog/bench_snapshot.py \
  --output out/shared-family-new \
  --live-worker out/shared-family/matrix-metal

# Separate diagnostic process; its oracle time is not a performance result.
python3 cell-assisted-egglog/bench_snapshot.py --skip-build \
  --output out/shared-family-new-checked \
  --live-worker out/shared-family/matrix-metal --live-check

python3 cell-assisted-egglog/shared-table-bench/summarize.py \
  out/shared-family-new/report.json \
  --checked out/shared-family-new-checked/report.json \
  --output out/shared-family-new/summary
```

This reuses the saved actual raytrace Pass-3 input and original packet at
`/3/repeat0/4`, after the real preceding passes/extractions. The helper checks
source hashes; it does not newly bake, collect a full trace, or invent a sparse
workload. Each lane runs once, no averages. Setup, continuous execution and
verification are separate; the original fork is not mislabeled pristine.

## Code map

- `src/table_live.rs`: one shared native row store, stable row slots, ruleset
  batches and original schedule; `Runtime::new_cpu` is the independent CPU lane.
- `src/family_program.rs`: typed atom wiring and shared index specifications.
- `family.metal`: generation-checked index lookup, exact guards and frontier join.
- `matrix.swift::FamilyExecutor`: one worker, combined command buffer, log
  compaction, lossless output growth and piggybacked acknowledgement.
- `egglog/src/cell_events.rs`: real row timestamps in bootstrap/commit events.
- `egglog/src/cell_assist.rs`: cached independent queries and shared RHS submission.
- `tests/table_live.rs`: delayed consumers across repeated GC, slot reuse,
  delete/reinsert, duplicate notifications, large Cartesian output, same-snapshot
  rule execution, watermarks, handback, union and stateful merges.

The old per-rule runtime measurements remain in `live-table-bench`; those artifacts
are historical comparisons, not results of the shared implementation.
