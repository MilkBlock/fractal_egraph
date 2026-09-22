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
Tier1 collection; it is an observer, not a replacement matcher. Both modes execute
all rounds, preserve their application histories in memory, and run final checks.
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
state copies, not merely hashes. No execution/history is skipped yet.

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

The measurement script now alternates baseline, old snapshot, and packet modes.
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
