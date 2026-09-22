# Hash rendezvous during native ripen

Run from the repository root, offline:

```sh
cargo build --release --offline
cargo run --release --offline -- ripen-probe out/convergence-demo 12 \
  experiments/ripen_convergence/a.egg experiments/ripen_convergence/b.egg
cargo run --release --offline -- ripen-probe out/convergence-baseline 12 --baseline \
  experiments/ripen_convergence/a.egg experiments/ripen_convergence/b.egg
python3 experiments/ripen_convergence/measure.py out/convergence-ablation
```

Use fresh output paths. `ripen-probe` runs actual native egglog with compact
Tier1 collection; it uses native matching and can reuse a previously completed continuation.
Baseline executes all rounds; reuse preserves each actually executed prefix and
attaches donor suffix evidence. Both execute the current entry's final checks.
The observer does not save intermediate graph JSON files. The comparison baseline
uses the same compact path with the observer disabled.

To enable the same run-local index in ordinary analysis:

```sh
EGG_LAYOUT_RIPEN_CONVERGENCE=1 cargo run --release --offline -- analyze \
  --recapture-tier0 --source YOUR_PROGRAM.egg --output out/convergence-analysis
```

Its aggregate counters appear in `saturated-rule-composition/queue.json`, and
per-cell events in `saturated-rule-composition/cells/*/ripen.json`. The existing
exact-entry cache remains in effect; cached cells do not produce new observations.
The index is disabled by default. Unsupported exported sorts/actions are recorded
as `unsupported`, not silently compared (notably the full math.egg f64 datatype).

## Packet fast path

The default observer now initializes from one exported seed per cell. Before the
existing trace drain it consumes committed `WriteEvent.actual` rows and union
outcomes, using native table schemas to decode their ordered typed arguments.
It does not treat logical matches or proposed writes as committed effects.

A flat fact store maintains:

- native value-to-local-variable bindings and union representatives;
- deduplicated `Ensure(op, args, result)` facts;
- reverse incidence from local values to facts;
- fixed named ports and primitive literal identities;
- an XOR fingerprint of `(record signature, multiplicity)` contributions.

Insertions update contributions directly; repeated writes do not change the set.
Union removes old contributions for incident facts and value labels, remaps their
variables/ports, coalesces duplicate facts, then adds new contributions. A constructor
key index propagates parent-result congruence when child unions make keys equal:
native rebuild can perform these unions without rule-caused union events. Canonical
representatives are chosen by incidence size; no recursive expression-tree copying
is needed. This signature is intentionally coarser than the old two-round graph
refinement and still requires exact comparison on hits.

Supported fast path: positive constructor/equality rules, with existing primitive
literals. Visibility effects, relations/mutable tables, or a newly introduced scalar
literal use the existing snapshot fallback, recording the reason. Their behavior
is preserved; there is no claim of an incremental implementation for them yet.
The tracker is discarded on any decoding failure, never partially trusted.

Normal packet execution has no per-round native graph serialization or table scan.
The initial seed and final saturated-state artifact still use the existing exporter.
When interning/comparing a new key, the flat mirror is compacted/copied into the
existing state representation; that work remains O(local state size). Exact
isomorphism remains bounded (10,000 search states, existing size limits).
Reverse-incidence union work is proportional to the affected facts and their arity,
including facts incident on the retained root whose port labels may change, and
transitive parent congruence; it can
still be large for a high-degree merged value. Fingerprint updates use ordered
multiset counters, so each affected contribution costs O(log number of signatures).

The index keeps one first-interned state per hash key, without bucket scans or
r-representative selection. Collisions and its 4,096-entry cap can lose recall;
exact state and contract checks prevent false sharing. Memory includes retained
state copies, not merely hashes. The current default can skip a certified completed continuation; see below.

Ablation and debug switches:

```sh
# Previous per-round whole-graph exporter:
EGG_LAYOUT_RIPEN_SNAPSHOT_PROBE=1 cargo run --release --offline -- \
  ripen-probe out/snapshot-probe 12 experiments/ripen_convergence/a.egg
# Validate the packet mirror against an independent native export every round:
EGG_LAYOUT_RIPEN_PACKET_AUDIT=1 cargo test --release --offline --test ripen_convergence
```

`convergence.snapshot_exports` counts detection-path exports (not the normal final
artifact); `audit_exports` counts extra debug exports. `packet_updates` counts
changed XOR contributions, not rule applications or inserted enodes. `packet_seconds`
measures committed-event decoding/update time. Index `signature_seconds` and
`changed_contributions` refer to the old snapshot path; packet work is in per-cell
counters. All scalar/visibility fallback reasons are reported.

The measurement script now alternates baseline, old snapshot, packet-only, and reuse modes.
`packet-results.json` records five-run medians for the current comparison:

| Case | Baseline | Whole-snapshot observer | Packet observer | Detection exports, old → new |
|---|---:|---:|---:|---:|
| A→B→C→D | 0.30 ms | 3.03 ms | 1.63 ms | 9 → 2 |
| Add-AC | 1.06 ms | 4.55 ms | 2.60 ms | 10 → 2 |

Hits and first-meeting rounds are unchanged on these examples; neither used a
fallback. The packet observer is cheaper than the old observer but still slower
than no observer. These results do not measure continuation-sharing acceleration.

## Previous snapshot-only results (local macOS, release)

`results.json` contains reproduction output. Timings are medians of the sum of
reported ripen-loop times for both cells, excluding process startup, initial setup,
and final export. Hash/comparison counters and timings are from the last observed
run, not medians. These tiny examples are mechanism probes, not a throughput study.

| Case | First hit in second cell | Remaining sweeps | Baseline | Observer |
|---|---:|---:|---:|---:|
| A→B→C→D, second entry already contains A=B | 0 | 3 | 0.34 ms | 2.99 ms |
| Add associativity + commutativity, different parenthesizations | 3 | 1 | 1.07 ms | 4.57 ms |

The chain gives an early meeting with a known earlier run's continuation. The
Add-AC case meets only when saturated contents are already materialized: the last
sweep confirms the fixed point. It does not demonstrate early algebraic convergence.
Remaining sweeps are observational opportunities, not measured saved work. Actual
skipped sweeps are zero. This implementation is slower, largely because it exports
and normalizes complete snapshots; hash preparation/update and exact comparison
in the final Add-AC run total about 0.14 ms and 0.24 ms respectively.

## Validation

```sh
cargo test --release --offline --test ripen_convergence
EGG_LAYOUT_RIPEN_CONVERGENCE=1 cargo test --release --offline \
  --test saturated_rule_composition_pipeline
```

Tests cover native early convergence and completed checks, ID renaming, different
contracts, subsume visibility, and a deliberate approximate collision: one 6-cycle
versus two 3-cycles have the same local refinement fingerprint but are not
isomorphic. The collision is rejected; no shared result is authorized.

Also ran the six other `saturated_rule_composition` tests and both pipeline tests
with the observer off. The existing paper test
`math_parenthesizations_share_body_but_keep_entries` fails because its fixture
contains `(run-schedule ...)`, while the existing `ripen` entry parser rejects run
commands. This failure occurs before the observer and is unchanged from the HEAD
parser; it is not suppressed or fixed by this experiment.

Packet regressions additionally compare every round with an independent native
export, including a child union that collapses two `F` parent rows and rewrites a
`Pair` key. The visibility example verifies explicit fallback. Both normal pipeline
tests pass with packet observation and differential audit enabled.

## Completed continuation reuse

Enabled by default in `ripen-probe`, and in compact analysis when
`EGG_LAYOUT_RIPEN_CONVERGENCE=1` enables the index. For packet observation without
skipping set `EGG_LAYOUT_RIPEN_OBSERVE_ONLY=1`.

A verified packet-state hit can now stop the current isolated ripen. Reuse requires
an already completed, checked, positive-constructor donor, the same fixed ports,
rule declarations/names and sweep schedule, and enough remaining round budget for
the donor suffix. Visibility/table fallback and artifact/history-export runs do not
skip. At most 64 completed engines are retained, in addition to the snapshot cap;
this is an entry cap, not a byte budget. No incomplete donor is reused.

The implementation clones the donor's saturated native engine, then runs the
CURRENT entry's checks on that independent clone. A different failing check fails
normally. It reuses the donor's complete exported body; there is no new matching
or action execution for the skipped suffix. `ripened.egg` still contains a complete
standalone replay schedule, which the test reparses and executes independently.

Tier1 retains only genuinely executed current-prefix applications. The report adds
`tier1.shared_continuation` with both intermediate interfaces, the verified value
map, donor source, donor Tier1 evidence, and the donor record interval after the
meeting. The donor prefix remains available to explain dependencies at the cut.
This is an explicit shared evidence edge, not a flattened list of newly executed
applications. Downstream code that wants expanded events must traverse that edge;
we do not rewrite the existing LayerStore's event IDs or native provenance IDs.
Donor evidence is stored in the run-level `continuations.json`, outside temporary
pipeline work folders. Each shared edge names its library, evidence ID, packet
suffix root and intermediate binding map. Reused results are not cached recursively, keeping it one hop.
The full donor evidence is stored once per retained donor; native engines are
still cloned. This is not yet a minimum-memory native execution representation.

`native_rounds` counts real current sweeps; `ripen.round` includes the certified
suffix. `convergence.actual_rounds_skipped` reports the difference. The boundary
kind `shared-continuation` is distinct from `ripen-round`. Current and donor trigger
origins remain separate. Current checks are never inferred from the donor checks.

`reuse-results.json` (five-run medians) shows:

| Case | Native sweeps without/with reuse | Actual skipped | Packet-only total cell time | Reuse total cell time |
|---|---:|---:|---:|---:|
| Chain | 7 / 4 | 3 | 5.52 ms | 5.35 ms |
| Add-AC | 8 / 7 | 1 | 8.77 ms | 8.54 ms |

Cell time includes parsing, initial/final exports, report generation, and completed
engine retention; process startup is excluded. These tiny differences are noisy,
not a demonstrated throughput improvement. In-loop time actually increases due to
engine cloning and evidence assembly. Both modes remain slower than no detection
(4.17 ms and 6.82 ms total cell time respectively). Add-AC only skips the final
fixed-point confirmation. The chain skips real generating sweeps as well.

Validation covers exact final-body equivalence against independent native execution,
standalone replay, no fabricated prefix applications, current failing checks,
remaining-budget admission, packet collisions/visibility, and both pipeline tests.


## Shared packet library (Ant-inspired)

`continuations.json` (version 2) contains `templates`, immutable `nodes`, and
`evidence`. Apply nodes reference a normalized effect template and an explicit
binding vector; Concat nodes contain only child IDs and cached length/hash/power.
Repeated fragments are hash-consed with exact key equality. Completed donors build
their packet trees with binary carries, and a meeting creates a shared suffix slice.
Construction currently happens when a donor finishes; this is not yet online
semantic shortcut learning before saturation.

`tier1.shared_continuation` no longer embeds `donor_tier1`, donor source, or donor
boundaries. Resolve its `evidence_id` in the named library, and interpret Apply
binding IDs in that evidence's `binding_space`. `donor_record_start/end` refer to
imported application records, while the packet slice uses separately recorded
`packet_boundaries`; these are intentionally different coordinate systems.
The library is persisted after a probe batch and after each analysis boundary.

```sh
cargo test --release --offline --test packet_library --test ripen_convergence
```

See [ant-reference.md](ant-reference.md) for pinned implementation references,
upstream differences, correctness boundaries, and measured structural examples.


## Semantic unification

Concat nodes now also carry separately indexed, sound root-position semantic
candidates when their source rewrites are supported. These retain intermediate
union effects and external guards and can be exported to executable egglog syntax.
They are not asserted to match the historically observed apply positions.
See [semantic-unification.md](semantic-unification.md) for the API, CLI, tests and
explicit supported fragment.
