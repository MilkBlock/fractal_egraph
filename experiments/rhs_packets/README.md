# RHS packets in the resident engine

This changes the **actual resident GPU publisher**. It does not create another
backend, change the input rules, bypass the analyzer's scope certificate, or
claim a Tensor Core/matrix speedup. Existing source markers and multi-stage
selection remain in force. The default small Pass 3 suite intentionally uses the
user-selected `resident-core` configuration (no source marker transformation).

## Four changes, in order

1. Ordinary/single-step actions stage requests directly from the frozen match
   buffer. They do not allocate a full `BC_WIDTH` Cell frame per binding.
2. An action program is split into movable constructor requests and an ordered
   residual. Admission is structural: no mutable RHS read, subsume, arbitrary
   callback, container, or arithmetic consuming a provisional identity. A pure
   constructor merge callback must use tables disjoint from the moved slice.
   Min/max and non-failing replacement callbacks also retain their exact order.
3. Constructor results can feed later constructors. A negative local reference
   identifies a request, not a provisional global e-class. Requests are interned
   by dependency depth against the frozen global graph and the batch's exact
   keys. Only after successful preparation are global IDs and rows allocated.
4. Analyzer-admitted pure constructor cones can stage multiple Cell steps
   together. Every successor read must have a complete, correlated witness in
   the seed or an earlier constructor. Runtime-hidden witnesses, partial frames,
   and unsupported successors keep the existing executor. Finite Run boundaries
   are not expanded into saturation. Function-update/union cones still use the
   existing continuation machinery; this is not universal Cell fusion.

The common path is now:

```
existing binding / selected Cell frame
  -> parallel exact request preparation
  -> local dependency levels + exact key interning
  -> one global reservation, parallel row publication and index insertion
  -> ordered residual set/union, or Cell slot backfill
  -> existing certificate, block union and rebuild machinery
```

Homogeneous cohorts make request offsets a multiplication. Preparation and hash
clearing share a dispatch; grouping also resolves sharing; publication also
updates the global index. The writer has 11 kernels, down from the first draft's
16 (the previous independent-constructor writer had 12).

At the existing completed command-buffer boundary, the host reads only the GPU
publication-ready flag. It omits the entire packet pipeline when no arena needs
it. A newly discovered cohort waits at state 9 until the next existing quantum;
no binding/table transfer or new explicit fence is introduced, but this can add
a control quantum. `packet_readiness_polls` and `packet_dispatch_quanta` expose
this scheduling tradeoff.

All inter-kernel dependencies stay on the GPU. A long dependency chain may need
another existing resident dispatch quantum; there is no new CPU request or
per-level host wait. This does **not** remove all barriers or serial work:
prefix offsets/reservation and noncommuting residual effects remain ordered.

## Identity, failure and visibility

Hash-table ownership may be chosen concurrently. Allocation rank is instead the
**earliest original request ordinal** for each exact key, making pure constructor
publication stable and preserving its ordered-reference allocation order.
Mixed callback programs preserve residual order and normalized graph semantics;
interleaved callback allocation IDs need not match the old executor.

Arithmetic is checked before global publication. Missing constructor prediction,
capacity shortage, or an unsupported Cell state switches the whole cohort to the
original ordered executor before publishing any request. Thus an overflowing RHS
still retains exactly its preceding effects. Packet-buffer growth restarts only
private preparation, without replaying already published bindings. Newly inserted
rows are integrated with the existing fact revisions, table versions, hashcons,
and constructor prediction store.

A hidden existing constructor remains hidden. In particular, it cannot certify
a future Cell step merely because its key was found. Union remains deferred to
the existing block boundary. There is no approximate key comparison.

The old path remains available via `lower(..., parallel_rhs_packets=False)`.
Saved v1 writer artifacts also remain executable. New artifacts use writer v2
and batch statistics v10. The existing 256-binding ordinary batching grain is also used for Cell
packets; smaller cones keep the existing Cell executor; this is dispatch granularity, not a matching/pruning limit.

## Reproduction

```
python3 -m unittest eggfuse.compiler.targets.metal.test_rhs_packets \
  eggfuse.analyzer.test_batches eggfuse.analyzer.test_regions eggfuse.ir.test_recipe
python3 experiments/rhs_packets/check.py \
  --build out/rhs-packets-build --output out/rhs-packets-check
python3 experiments/rhs_packets/bench.py \
  --build out/rhs-packets-build --output out/rhs-packets-bench
cargo profile
python3 experiments/rhs_packets/report.py
```

The build directory requires the existing typed `frontend`; the test runner
builds the real resident Swift worker. Test outputs are immutable per invocation.
`check.py` checks the native graph (facts, values, equivalence and visibility),
GPU repeatability, pure-constructor full state/history equality, ordered custom
merge, late union, constructor prediction, hidden intermediates, finite visibility,
private buffer growth, and overflow with retained preceding effects.

`bench.py` measures one same-worker off/on pair, excluding shader setup. It is a
large synthetic nested-constructor workload, **not** Raytrace or Pass 3.
`cargo profile` runs the three complete small Pass 3 inputs and reuses cached
historical evidence. Profiled encoder times must not be treated as clean times.

Counters 60–70 distinguish binding requests, fresh rows, dependency passes,
ordered tails, fallback cohorts, fused Cell contexts/steps, and existing-key hits.
Logical actions, requests, and fresh rows are intentionally separate metrics.
`packet_reused_requests` counts ordinary hashcons/deduplication (requests that
needed no fresh row); it is **not** a RipSatCell cache-hit or closure-reuse score.
