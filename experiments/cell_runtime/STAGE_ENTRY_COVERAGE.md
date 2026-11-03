# Preserve entry coverage and share exact seed predicates

Follow-up: [direct commit receipts](COMMIT_RECEIPTS.md) removes duplicate
capture lookup using original query/action witnesses. The measurements below
retain the preceding keyed-capture implementation.

This follows [STAGE_SPINE.md](STAGE_SPINE.md), using the same authenticated
sum-bits source_sk_mk and standalone, single-thread Cell CPU engine. The target
of this experiment is **fewer candidate validations**. It does not link native
egglog, run on GPU, change source effects, or improve the older terminal Cell
closure certificate. Merge programs remain ON.

The implementation reduces inclusive candidate checks **86,432 -> 34,279
(60.34%)**, with fewer actual row decodes. It still takes **134.117 -> 169.005 ms
(26.01% slower)** than the ordinary path in the final paired run. All new modes
remain opt-in. Times from earlier revisions are preserved, not mixed with this
control or selected for the fastest baseline.

## Exact entry coverage, with residual completion

Previously a rule lost all prepared Spine entry work when any pending pivot
lacked context or required inverse/multivalued completion. A partial answer now
contains both its bindings and an exact set of covered `(query atom, witness)`
entries. Ordinary matching evaluates only the gaps. Both sets of answers merge
through the existing complete-input binding map before RHS execution.

The entire pending delta remains available for semi-naive ownership. Coverage
is **not** a set of physical witnesses alone: one row can be a complete entry
at one position of a self-join and require ordinary completion at another.
An explicit native-backed test first consumes old `F(x)` rows, creates a new
`F(F(x))`, then verifies that its unsupported second pivot still finds the old
predecessor. Guard failures and literal/alias rejection also cover their exact
entry without claiming that other entries are empty.

Dispatch first checks witness/table headers and typed key-flow eligibility.
Unknown entries do not get decoded and rebound a second time merely to decide
that they need ordinary matching. Missing context and unsupported key flow are
counted separately. There are no candidate, depth or memory cutoffs.

This expands the real-case Spine path from **2,637 to 3,775 resumed bindings**,
with 279 mixed-coverage queries, paths up to 12 steps, and shared contexts serving
up to 9 rulesets. These are logical bindings/continuations, not new mutations.
However, partial coverage alone does not reduce the inclusive candidate count:
it mainly changes which executor checks an entry.

## Share the seed's verified predicate, including residual routes

A typed table atom defines an exact entry predicate: table identity, constants
at their original column positions, and repeated-variable equalities. Variable
names/slot numbers normalize by first occurrence. Each consumer also has an
explicit projection from the checked row's columns to its own input slots.

Within one original Run's immutable LHS snapshot:

1. Decode each requested witness once into a shared temporary row.
2. For each distinct `(predicate, witness)`, validate the columns once and cache
   success or failure. This retains a checked input tuple, not a complete query
   answer or a guessed endpoint.
3. Another consumer with the same predicate reuses that validation, projects the
   fields into its own slots, and runs its own suffix, guards and effects.
4. Extend this sharing to ordinary residual/direct entries too. A rule need not
   have a prepared producer context to reuse the same seed predicate.

Constants, column order and alias requirements participate in identity. Different
predicates may share row decoding, but never share a predicate verdict. No
fingerprint is used as an equality proof. Per-rule watermarks and the complete
current delta domain remain consumer-specific.

The shared rows/verdicts expire at Run publication and on epoch changes. All LHS
queries run before any RHS, and no cached snapshot crosses mutation/repair into
a later Run. This handles union, function updates, hiding and deletion without
reusing a stale successful or negative verdict. Checkpoint resume starts cold.
The existing source binding order, full RHS, custom merge order and changed
flags remain unchanged.

This is exact query-entry common-subexpression reuse. It does not add a new
logical consequence to the egraph. It can benefit a rule with no producer Cell
context, as a native-backed regression explicitly checks. Its savings must not
be attributed to terminal Cell certification or to avoided inner joins.

## Candidate accounting and the final four-way measurement

Raw output: `out/stage-spine-entry-final`. All four lanes use one
binary/program and original source schedule. Each lane is measured once with
profiling/audit/builds/tests excluded. Three later audits check the transitions
ordinary -> shared, whole-query -> partial, and partial -> shared separately.

The old `total_candidates` field omits direct rule seeds and Spine probes.
This experiment reports:

```
all_candidate_visits = ordinary delta/residual candidates
                    + compiled merge candidate rows
                    + direct rule candidate seeds
                    + Spine/shared-entry predicate checks
```

Exact-key/port reads, header dispatch and cached-verdict routing are separate
costs. Cache hits still execute routing/projection; they are not free. The
reduction below means actual candidate column validation and decoding removed,
not removal of every per-consumer request.

| Metric | Ordinary | Whole-query Spine | Partial entries | Shared entries + residual routes |
| --- | ---: | ---: | ---: | ---: |
| Legacy ordinary/residual + merge count | 84,033 | 80,450 | 77,648 | 17,330 |
| Direct seed checks | 2,399 | 427 | 60 | 0 |
| Spine/shared predicate checks | 0 | 9,237 | 8,724 | 16,949 |
| **Inclusive candidate checks** | **86,432** | **90,114** | **86,432** | **34,279** |
| Decoded query rows | 113,887 | 117,119 | 110,973 | **59,961** |
| Exact-key lookups, capture included | **62,212** | 81,809 | 81,412 | 81,412 |
| Inference | **134.117 ms** | 168.350 ms | 164.135 ms | 169.005 ms |
| Peak RSS | **204.22 MiB** | 208.84 MiB | 208.41 MiB | 207.89 MiB |

The final cache processes **69,102 consumer entry requests** with **16,949
actual predicate checks** and **52,153 verdict reuses**. Of these reuses,
**41,965** follow a different consuming rule; 10,188 are same-rule reuses.
There are **50,343 shared row hits**, 18,284 unique root row decodes across Runs,
and a peak of 1,029 cached predicate verdicts. These counts do not measure RSS;
the process RSS is reported independently above.

Reuses by receiving rule group: original source consumers **43,081**, structural
key helpers **7,860**, marker publishers **1,212**. The largest source-set totals
are always-run **18,285**, type-analysis **5,019**, error-checking **3,897**, and
context **2,772**. This is routing attribution, not independent marginal Cell
credit. Per-rule requests and first checks are saved in `stage_entry_results.json`.

The final comparison keeps all **12,500 logical bindings**, **19,188 constructor
requests**, **3,210 union requests / 1,039 effective unions**, **951 repair
collisions**, and **1,496 source Runs**. Full state is **4,314 rows / 2,324
eclasses**. Each of three paired audits passes **1,497 exact snapshots**, including
all auxiliary tables, function values, equivalence, visibility, complete binding
lists, Run paths, iterations and changed flags. Fresh-process replay is exact.
These are standalone-engine comparisons; whole-Pass-3 raw-native equivalence
and actual extraction quality are not newly established.

## Depth, remaining work and limits

A separate diagnostic replays the frozen timed configurations, checks their
binary/program fingerprints, candidate totals and exact endpoints, and records
the existing residual-depth profiler. It does not repeat or use instrumented
timings. `stage_entry_depth.json` preserves each original rule's read counts,
maximum-depth histogram, refutation counts and conditional survival curves.

The profile covers **remaining matched ordinary delta seeds and inner table
reads**. Singleton point projections, direct rules and shared seed predicates
are outside that depth profile. Its seed count changes with query routing;
removed queries are not automatically new refutations.

| Inner table depth | Ordinary | Partial | Shared |
| --- | ---: | ---: | ---: |
| 2 | 9,564 | 9,564 | 9,564 |
| 3 | 5,342 | 5,342 | 5,342 |
| 4 | 1,154 | 1,154 | 1,154 |
| 5 | 739 | 739 | 739 |
| 6 | 273 | 273 | 273 |
| 7–12 | 258 | 258 | 258 |
| **Total inner candidate rows** | **17,330** | **17,330** | **17,330** |

Ordinary matched residual starts change 65,859 -> 59,668 because prepared
functional entries leave this profile. Partial and shared have identical
remaining seed populations, per-rule depths and inner rows. Conditional
`Pr(max_depth >= 2)` changes 45.61% -> 50.35%; this is a change in composition,
not evidence of stronger pruning. Original-rule-weight curves on common support
cover 98.10% of original matched-seed weight and are also preserved. Fixing rule
weights still does not fix candidate identity.

**No inner enumeration was reduced in this experiment.** The next opportunity
is retaining/versioning complete suffix domains or failure evidence, especially
the always-run Concat/SubTuple rewrite (1,995 inner visits in this input).
A wider baked recipe is useful only if it replaces this remaining repeated work.
Capture still performs 20,129 global key lookups; both partial and shared have
6,208 current-port refreshes. The 60.34% predicate-check reduction does not yet
pay for capture, canonicalization, cache lookup and projection in elapsed time.

Earlier attempts are retained in `stage_entry_results.json`: partial entry
routing alone, and sharing only producer-covered entries. The latter removes
3,086 validations, whereas sharing ordinary residual entries too removes 52,153.
A subsequent draft also temporarily allocated reference-counted rows on the
ordinary path. The final representation keeps ordinary rows owned and shared
rows reference counted. That draft's complete four-lane measurements are
preserved separately; different revisions' absolute times are not used to
attribute a speedup.

## Validation and reproduction

Runtime discovery: **202/203 pass**. The one preexisting failure remains
`ReferenceTests.test_marker_multiple_owners_union_late_consumer`, expecting
`distinct_delivery_events < delivery_events` with values `16 < 16`. Its graph
and binding comparisons pass; the unchanged parent reproduced this failure in
the preceding stage-Spine work. The new suites contain 15 stage, 18 partial and
21 shared-entry tests, including native-backed fixtures and the existing semantic
corpus. Three checkpoint tests were additionally checked with their exact modes
explicitly enabled. All **13 compiler/IR tests and 7 Rust unit tests pass**.

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml \
  --bin eggfuse-cell-runtime --example project_stages --example project_semantics

EGGFUSE_TEST_FRONTEND=/tmp/eggfuse-ripen-build/debug/examples/eggfuse_frontend \
  /opt/homebrew/bin/python3 -B -m unittest experiments.cell_runtime.stage_spine_test \
  experiments.cell_runtime.stage_partial_test experiments.cell_runtime.stage_shared_test

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --compiled out/stage-spine-compile --feature stage-shared --stage-ablation \
  --output out/NEW --audit

# Diagnose those same lanes without repeating timing:
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --profile-existing out/NEW/results.json --output out/NEW-DEPTH
```

Direct flags: `CELL_CPU_STAGE_SPINE=1`, `CELL_CPU_STAGE_SPINE_PARTIAL=1`,
`CELL_CPU_STAGE_SPINE_SHARED_ENTRIES=1`. The partial/shared flags default to OFF;
they do not rewrite or retrain the authenticated artifact.
