# Reuse Graph: contracts and offline Bake library

Implementation: `src/reuse_graph.rs` and `src/reuse_library.rs`. These provide
data-model/accounting contracts and explicit offline native-artifact import. It does **not** yet discover templates, validate a graft's
semantic applicability, install a macro, modify the matcher, or virtualize facts.
Native preparation now exports a typed origin recipe and interface terms;
execution and planner behavior are unchanged.

## Names and identities

- **RipSatEgraph** is an immutable shared body. The legacy
  `SaturatedRuleComposition` DTO also represents entry/prefix snapshots; the new
  wrapper requires a native report with `Saturated`, no interrupted step, passed
  checks, and matching exported sizes. It rejects observation-only prefixes.
  This checks trusted local artifact metadata, not an independent mathematical
  proof or an adversarially supplied report.
- **EnclosureDescriptor** refers to a canonical body, upper/lower interface slots,
  and an origin recipe using the existing `Comb` / `RelativeBinding` vocabulary.
  Recipe parents are local topological indices. Alias/effect symbols share one
  namespace across recipe steps, preserving cross-step external identity. Rules retain their original
  guards in their full rule text; semantic replay validation remains the native
  verifier's responsibility. Structural interning is not general equivalence of
  arbitrary triggers.
- **EnclosureInstance** binds a descriptor in one run. Actual values are separate
  from the descriptor key. A sparse fact-slot map uses run/table/fact/version
  identities, not only eclass equality. Canonicalization and native existence
  checking must be supplied by the runtime adapter in a later phase.
- **Graft** stores an applied event, its causal episode, boundary ports and actual
  fact reads. The module does not infer `unexpected` from connectivity degree.

Exact body interning reuses the existing typed-state comparison and returns an
incoming-to-stored value map. Callers must translate interface value slots through
that map. Different trigger/origin descriptions can reference one body. Descriptor
and instance indexes share their immutable payloads rather than keeping two copies.

## Connectivity and counters

Upper/lower degree counts distinct **formal interface slots**. Repeated references
to one slot count once; two ports aliased to one actual value still have degree 2.
Internal usage counts distinct actual row/version identities among the observed
body-row slots. Eclass equality alone does not merge two different e-nodes.
No transitive ancestors are added to the footprint.

The three counters are:

1. `reuse_count`: explicit adopted-reuse events, additionally separated into body
   storage, internal derivation and query-plan reuse. A lookup, registration or
   weak-dominate candidate does not increment it. Events are deduplicated per
   instance/event/kind and cannot be reassigned to a different revision.
2. `unexpected_break_count`: one per instance/causal episode, even if follow-on
   applications cross revision changes. Individual graft events are retained.
   Expected connections do not increment it.
3. `new_enclosure_count`: a newly interned runtime supplement, not an initial Bake
   definition or a new parameter instance. A new trigger referencing an existing
   body still counts as a new enclosure, without copying that body.

Changing an instance revision clears its fact-slot mappings. Stale evidence,
conflicting event payloads, wrong-run events, invalid slots/types and inconsistent
required aliases are rejected before counters change. This does not yet implement
native union subscriptions, instance re-canonicalization or guard validation.

## Tests and evidence scope

```sh
cargo test --offline --test reuse_graph
```

Synthetic model tests cover equivalent-body maps, distinct origins, normalized
symbol aliases, parameter-instance separation, event/episode deduplication,
revision invalidation and actual-fact aliasing. The native integration test runs
one finite program and DABCABC through the existing analyzer and checks that only
the completed artifact can enter a RipSatEgraph. It is not a planner speedup or
storage-compression benchmark.

`report()` is an inspection snapshot, not a resumable library file: restoring
runtime event indexes is not supported. The separate Bake library persists static
bodies, descriptors and source/origin evidence; it never restores runtime counters.

## Next stages

1. **Done:** objects, typed references, structural checks and accounting.
2. **Done (offline):** Bake library import/export and native origin-recipe extraction;
   this captures the selected composed entry, not a complete local saturation trace.
3. Conservative fragment filters (unknown ports return Maybe).
4. Runtime graft observation and supplementary learning, initially without changing
   native execution results.
5. Verified storage-reference/recipe selection.
6. Planner query reuse with full binding-coverage checks.
7. Workload-guided boundary changes; only then selected storage virtualization.

## Timing regression benchmark

```sh
# Establish a baseline once on this machine (do not overwrite it during checks).
REUSE_BENCH_OUTPUT=/tmp/reuse-base.json cargo bench --offline --bench reuse_graph
# After a change:
REUSE_BENCH_OUTPUT=/tmp/reuse-current.json cargo bench --offline --bench reuse_graph
python3 tools/bench/check_reuse.py /tmp/reuse-base.json /tmp/reuse-current.json
```

The checker exits 1 when any median regresses by more than 20%; use
`--threshold 0.10` for 10%. Missing benchmarks or incompatible metadata fail
rather than silently skipping checks. Run on the same idle machine with the same
Rust toolchain and build profile; OS/architecture checks cannot detect all machine
or load differences. Repeat a failed comparison before attributing it to code.

The harness uses 5 warmup batches and 30 measured batches of 128 calls each.
Fixture setup and destruction are outside timing; API allocation, argument clones,
and incremental index growth within a batch are included. It covers exact body
lookup at catalog sizes 1/64/256, descriptor lookup, instance lookup/insertion,
graft insertion and adopted-reuse insertion. Synthetic bodies have two rows;
this guards API overhead and catalog scan cost, not large-graph isomorphism,
native ripen throughput, memory growth or end-to-end analysis latency.

No additional dependencies or network access are required. Baseline JSON is local
and deliberately not automatically refreshed. Test the checker with
`python3 -m unittest discover -s tools/bench -p 'test_*.py'`.

## Build a library from native runs

```sh
EGG_LAYOUT_GRAPHS=0 cargo run --release --offline -- analyze \
  --source YOUR.egg --output out/new-run
cargo run --release --offline -- reuse-bake out/library.json out/new-run
cargo run --release --offline -- reuse-library-check out/library.json
```

Multiple run directories may follow the output filename. The output must not
exist. `reuse-bake` prints counts and a per-cell skipped reason; older artifacts
without `reuse_recipe` must be recaptured. Unfinished or unsupported cells never
enter the saturated body table. Bodies are stored once, descriptors reference
body IDs, and each source/origin record remains attached to its descriptor.
Initial admissions and library reload do not count as runtime reuse.

Upper ports are equality-valued outputs of the selected root occurrence; lower
ports are extracted opaque RipenInput parameters, in declaration order. Ground
interface expressions are parsed with egglog and resolved against the exported
state, including union aliases. Missing or ambiguous terms are rejected, not
silently replaced with all body nodes. Cut producer dependencies become external
bindings and missing effect obligations. This conservative recipe is not proof
minimization. Staged injection evidence and full source remain in each entry.

This is trusted local artifact import with structural validation, not a sandbox
for untrusted libraries, a fresh semantic replay, or a proof that arbitrary new
bindings can execute the recipe. The library does not install rules in tier0.
The v1 JSON is a machine interchange/debug format, not an executable `.egg` DSL.
