# Automatic shared long-range touchpoints

2026-10-09. This extends the shared operator Program with automatic cuts driven
by multiple consumers, correlated touch rows, and exact joins between instances.
No benchmark or egglog engine modification is part of this change.

## The rule for choosing a touchpoint

Inputs are a shared typed operator DAG and named consumer output requirements.
No internal port, chain length, split depth, or score is provided to discovery.

1. Propagate each consumer's demand backward, as a bitset on each operator.
2. Group connected operators used by exactly the same two or more consumers.
3. Export every requested root and every value used outside the component.
4. Preserve all imported values and the root-input bindings on which they depend.

Inputs and constants are not component members; a shared literal alone does not
merge otherwise independent computations. Components are maximal under this
specific structural criterion, not globally optimal interfaces. An unused or
single-consumer computation does not become a shared touchpoint. Every shared
operator has exactly one owner in a plan, and all cut edges remain represented.
There is no candidate enumeration, length cutoff, rank threshold or search budget.

Demand propagation uses consumer bitsets. Component traversal is linear in the
visited graph after labels are known. The bitset cost depends on consumer count,
and computing/storing transitive input-support sets can be much larger than the
operator graph. Packet size and materialized join output are not constant-cost.
The algorithm is a template-level decomposition, not a per-history plan search.

## Large synthetic shape: automatic 48/16 split

Given two independent 64-stage chains and consumers that compare the final
outputs, add the final outputs, and compare the stage48 outputs, discovery finds:

    left input -> [stages1..48] -> [stages49..64] -> final
                       |                              |
                       +-> stage48 consumer           +-> final consumers

    right input -> [stages1..48] -> [stages49..64] -> final

There are four touchpoints with144,48,144,48 operators. All384 shared operators
are accounted for once. The source graph and its existing cell aliases are
unchanged. Adding consumers and rediscovering produces a new plan; previous
plans remain valid for their original consumer requests.

These chains are a scalar Program fixture, not automatically mined egraph Cells.
Tests include signed arbitrary-precision inputs so the long chains are not all
constant due to their min cap. Those signed cases are interpreter-only; the
Metal ABI has the separately checked unsigned range from the prior experiment.

## Joint binding, not independent projections

A Packet is one instance's complete fragment boundary row. It includes:

    fragment identity + instance key
    imported/output values
    root-input binding snapshot

For example, rows (3,4) and (30,40) do NOT imply that (3,40) exists. `join_packets`
uses the joint tuple of linked ports. It retains both original packets as the
witness of each match, including different instances with identical values.
All rows must be fresh, even if they do not find a partner. Port kinds must
agree; Python's True==1 does not make bool/int ports interchangeable. Each join
side uses one Program port namespace. Links are explicit consumer equalities;
this operation does not infer how arbitrary fragments should connect.

A Joined witness can feed a new Program consumer through exposed input/output
ports. Neither source Cell is copied. Freshness is checked again before observing
that consumer. This is composition in the supported pure scalar domain, not a
proof of equivalence between arbitrary egraph subgraphs.

## GPU / CPU outputs become touch packets directly

`TouchRequest.capture(plan, runtime, instance_key)` records the needed input
snapshot and defines the automatically selected output roots. These roots are
lowered using the same existing CPU/Metal compiler.

After backend execution, `request.finish(runtime, values)`:

* rejects changed supporting inputs, missing/extra outputs, or wrong value types;
* recovers imports from input bindings, constants, and other fragment exports;
* produces correlated Packets without evaluating the DAG or populating its cache.

The backend arithmetic is trusted after code-generation verification; finish
checks schema and freshness rather than recomputing the calculation. Unrelated
input changes do not invalidate the request. Packet freshness is deliberately
conservative: a changed support input invalidates its packet even if the final
output stayed equal. Re-exporting can still reuse unchanged runtime values.

## Integration and checks

```sh
python3 experiments/cell_kernel/test_touchpoints.py
python3 experiments/cell_kernel/test_program.py
python3 experiments/cell_kernel/verify_shared.py --gpu
```

24 touchpoint tests pass, covering graph-cut invariants, independent arithmetic
references, update sequences, joint-row joins, staleness, input kinds, consumer
extension, and backend packet reconstruction with evaluation disabled.
The existing12 Program tests also pass.

The source-checked CICM size/selection pilot now automatically discovers three
shared fragments (2,3,2 operators) for size export, eligibility, and its additional
internal consumer. Four exported values are generated by both CPU and real Metal
execution for32 instances. Reconstructed packets match interpreter-produced
packets exactly, while source runtime caches remain unchanged. Native effect and
full-binding comparisons from the prior pilot still pass.

Artifacts:

* touchpoint-checks.json: decomposition and joint-row evidence, no timings.
* shared-verification.json: CPU/Metal/native correctness results.
* out/cell-shared-program/automatic-touchpoints/: generated programs and plan.json.

Implementation is split into three small modules:

* touchpoints.py: discovery and correlated snapshots.
* touch_join.py: explicit typed joint-key composition.
* touch_backend.py: snapshot tickets and compiled-result reconstruction.

## What is not claimed

Consumer roots still come from a known Program or source lowering. Discovery
finds shared long-range dependencies inside that representation; it does not
translate arbitrary .egg patterns or discover all useful semantic abstractions.
There is no optimality or production speedup claim. It does not implement general
union/congruence/subsume, provenance for historical egraph mutations, automatic
result eviction, or incrementally maintained joins between packet batches.
Caller-provided instance keys and canonical boundary values must have the correct
scope. Current scalar kinds are int/bool, not the full eggcc sort system.
