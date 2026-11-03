# Compiler-generated shared entry node scheduling

This implements the next step after [commit receipts](COMMIT_RECEIPTS.md): a
changed input updates shared entry parameters once, and original consumers borrow
those parameters at their legal source Runs. It is a **generic typed entry-node
plan for the standalone, single-thread Cell CPU engine**. There are no operator,
rule or ruleset name cases. It does not link native egglog, run on GPU, or add a
terminal Cell closure certificate.

In the final single-run paired sum-bits Pass 3 measurement, inference is
**157.853 -> 146.952 ms (6.91% lower)**. Candidate checks including node maintenance
fall **34,279 -> 25,643 (25.19%)**. Only this full performance workload was measured,
so `CELL_CPU_SHARED_NODE_SCHEDULE=1` remains opt-in. The source query suffixes,
full RHS operations and original scheduling remain authoritative.

## The compiled contract

`eggfuse.analyzer.entry_schedule.build` derives `entry_schedule` from the complete
Recipe IR. The Cell CPU target emits `entry-schedule.json`, includes it in the
artifact manifest/capabilities, and fingerprints the pass implementation. The
source control program and Recipe checksum are part of the plan. Rust separately
reconstructs the typed normalization/projection relation and rejects a changed
node, omitted consumer or forged projection even after an artifact is rehashed.
It validates an emitted plan; it does not discover an alternative plan at runtime.

A node describes one exact table entry predicate:

- actual table identity and original column positions;
- constants and repeated-variable equalities;
- variables normalized by first occurrence;
- shared parameter coordinates and explicit consumer slot projections.

Different predicates can share physical row decoding, but not a predicate verdict.
A consumer can rename/reorder its variables without changing the shared predicate;
its projection still connects the correct columns to its own slots. Naive rules
and source probes keep their ordinary replay paths.

The authenticated final artifact contains **297 entry nodes for 2,302 consumer
atom positions**, with 223 nodes used by multiple consumers. This is a static
plan size, not the number of live Cells or newly committed facts. The public
compiler produces it through the same SK/Bake/marker/Recipe pipeline; a second
identical invocation reuses all seven passes while validating their outputs.
Old artifacts remain accepted with the mode OFF. Enabling it without an emitted
entry schedule requires re-emission, rather than silently rebuilding a new plan.

## Changed inputs update shared state

After original RHS execution, merge, repair and storage absorption, the scheduler
records current changed/removed witnesses. Exact same-key forwarding also supplies
retired function/fact handles, including when no live row remains under the old
handle. Its subscriptions are grouped by node, not duplicated as per-rule pending
witness sets. The ordinary dirty-rule flags remain source-gated.

A node maintains the current complete set of correlated entry tuples and pending
changed witnesses. On first demand it scans the current table; subsequently it
refreshes only notified witnesses. Multiple changes before any legal consumer
Run coalesce to the latest state: no consumer could observe the skipped states.
Updates are evaluated once when the first legal consumer needs that node; later
consumers reuse its current results.

Packets preserve current witness identity, row values and epoch. When the exact
row parameters are unchanged, an immutable parameter vector can be retained while
the new epoch still enables the required source application. The native-backed
regression changes an input away and back before consumption and verifies that
an effectful counter still fires again.

Current result epochs have a separate ordered index. Each consumer selects only
rows new enough for **its own** rule watermark; a fast consumer does not consume
a slow consumer's delta. This index contains current tuples, not historical
versions. Literal equality identities conservatively force a complete node
refresh after union, including inputs whose old predicate had failed.

All query bindings for one Run are still gathered before any RHS is exposed.
Removed/subsumed rows retire their packets on refresh; new alias alternatives
remain correlated and all residual reads/guards stay enabled. Full source Run,
Repeat and Saturate behavior, action ordering, merge behavior and changed flags
remain unchanged. Checkpoints store authoritative graph/watermarks, then rebuild
these optional caches cold without replaying old effects.

## Consumers borrow parameters

The old path made a request for each rule/pivot/witness, allocated a private
full-width input array, and copied the seed fields into it even on a shared
predicate-cache hit. The new source-gated delivery contains references to a
shared packet and the compiled parameter projection.

A query workspace reads through this immutable projection and stores only its
private suffix writes in an overlay. Source guard checks and binding comparisons
see the same values through either representation. The full original binding is
still materialized when needed for RHS execution; it cannot be omitted merely
because a seed is shared. Existing stage-Spine functional continuations also
consume these projected seeds, retaining the same cursor/tie-break behavior.

This first compiled scheduling layer shares **table entries**. It does not claim
to have compiled every multi-atom suffix or inverse join into a shared DAG. The
existing functional recipes and exact residual matcher execute those suffixes.

## Final paired measurement

Raw output: `out/shared-node-final`. Input: public compiler output
`out/shared-node-compile`. Same program/binary, merge programs, stage Spine,
partial coverage, shared entry validation and commit receipts; only the shared
node schedule flag changes. Each lane ran once, without concurrent builds,
tests, audits or diagnostics. Setup, process wall/RSS and fingerprints are stored
in `shared_node_results.json`; neither the first attempt nor an older baseline is
selected as a faster control for the final result.

| Metric | Per-consumer requests | Compiled shared node schedule |
| --- | ---: | ---: |
| Inference | 157.853 ms | **146.952 ms** |
| Legacy per-entry request calls | 69,102 | **0** |
| Consumer shared-view acquisition calls | 0 | 6,045 |
| Shared parameter-view deliveries | 0 | 68,153 |
| Initial entry parameter value copies | 201,700 | **22,957** |
| Entry predicate checks | 16,949 | **8,313** |
| All candidate checks, maintenance included | 34,279 | **25,643** |
| Query row decodes, nodes included | 59,961 | **50,847** |
| Exact-key lookups | 61,341 | 61,341 |
| Capture key lookups | 58 | 58 |
| Peak RSS | **216.53 MiB** | 217.17 MiB |

The parameter-copy counters count actual initial entry assignments, including
partial assignments before predicate rejection and shared-node maintenance.
They do not count every `Value::clone` in suffixes, key construction or final RHS
binding materialization. Entry copies fall **88.62%**; consumers still inspect
and use 200,400 borrowed slot values. The remaining 68,153 deliveries are real
continuation work, not zero-cost or eliminated consumers.

The shared scheduler receives 9,539 changed-witness headers, creates 9,940 node
notifications, refreshes nodes 1,348 times, and performs 8,313 current predicate
checks. It decodes 9,170 root rows rather than 18,284 in the prior path. Its 950
cold-scan rows and invalidation/retirement work are included in these counts.
There are 700 retained parameter packets across refreshes; 4,033 current packets
remain at the endpoint. Logical packet counts are not physical RSS measurements.

| Phase | OFF | ON |
| --- | ---: | ---: |
| Search, including node refresh/delivery | 75.398 ms | **68.813 ms** |
| Event delivery and GC | 7.478 ms | **2.410 ms** |
| Index/context publication | **19.576 ms** | 20.245 ms |
| Original RHS, capture included | **34.319 ms** | 35.150 ms |
| Repair | 12.572 ms | **11.848 ms** |

The direct goals are fewer per-rule pending inserts, predicate-cache requests,
initial parameter copies and repeated root evaluation. Phase timings are one
sample; changes to unchanged repair/RHS work are not independently attributed.
No broad performance result beyond this workload is claimed.

The first implementation measurement was 162.925 -> 154.011 ms (5.47% lower).
The final revision adds explicit entry-copy accounting. Both results and their
binary fingerprints are retained; they are not averaged or chosen best-of runs.

## Depth and semantic validation

A diagnostic replays the frozen final configurations without repeating timing,
checks their exact endpoints and candidate totals, and saves every original
rule's depth counts in `shared_node_depth.json`. The two remaining-matcher
profiles are **exactly equal**, including per-rule fields:

- 59,668 matched ordinary residual starts in both lanes;
- 17,330 inner candidate rows in both lanes;
- depth 2: 9,564; depth 3: 5,342; depth 4: 1,154; depth 5: 739;
- depth 6: 273; depths 7–12: 258;
- identical deepest-seed histograms, refutation counts, survival curves and
  baseline-rule-weight curves.

These depth curves exclude shared entry maintenance, direct rules and singleton
point projections. They are not native egglog Free Join profiles. No inner
search pruning or full candidate-identity causal effect is claimed. The 8,636
fewer inclusive checks are entirely entry-layer work; exact residual computation
remains unchanged.

All **1,497 stage snapshots** match exactly OFF/ON: full tables including marker
auxiliaries, function values, equivalence, visibility, complete binding lists,
Run paths, iterations and changed flags. A fresh ON process reproduces exact
identities. There are still 12,500 logical bindings, 19,188 constructor requests,
3,210 union requests / 1,039 effective unions, 951 repair collisions and 1,496
source Runs. Endpoint: **4,314 rows / 2,324 eclasses**. The existing 3,775 resumed
Spine bindings and 12-step maximum chain remain. This does not establish the
previously unresolved raw-native whole-Pass-3 comparison or new closure authority.

24 shared-schedule tests pass, including native-backed renamed consumers,
independent watermarks, fixed equality identities, function coalescence, original
Run isolation, effectful value-return updates, index reset, mode independence,
legacy/tampered artifacts and cold checkpoint restore, plus the existing semantic
corpus. 29 compiler/IR tests and 7 Rust unit tests pass. Runtime discovery is
**247/248 passed**, retaining
the preexisting `ReferenceTests.test_marker_multiple_owners_union_late_consumer`
assertion expecting `16 < 16`; its preceding graph/binding checks pass and the old
parent previously reproduced it. That assertion is unchanged.

## Reproduction

Re-emit through the public compiler to include the validated node plan:

```sh
/opt/homebrew/bin/python3 -B -m eggfuse.compiler \
  --source out/stage-spine-compile/source.egg \
  --bake-source out/stage-spine-compile/bake-source.egg \
  --tools /tmp/eggfuse-ripen-build/debug/examples \
  --solver-python /Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python \
  --backend cell-cpu --multi-stage-cell --stop-after device \
  --output out/shared-node-compile

cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml \
  --bin eggfuse-cell-runtime --example project_stages --example project_semantics

EGGFUSE_TEST_FRONTEND=/tmp/eggfuse-ripen-build/debug/examples/eggfuse_frontend \
  /opt/homebrew/bin/python3 -B -m unittest experiments.cell_runtime.shared_schedule_test

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --compiled out/shared-node-compile --feature node-schedule --output out/NEW --audit

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --profile-existing out/NEW/results.json --output out/NEW-DEPTH
```

Direct mode: `CELL_CPU_SHARED_NODE_SCHEDULE=1` with a new Cell CPU artifact.
The scheduler also works independently of stage Spine. Naive rules, probes and
non-delta query modes retain ordinary execution. This is an optional CPU lowering;
the GPU regions path has not been switched to this entry-node schedule.
