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

## Mechanism and scope

- Observe initial setup and completed native ruleset sweeps, after rebuild.
- Flatten the exported typed row graph. Fixed ports, literal values, ordered
  argument roles, local aliases and visibility contribute to two rounds of
  ID-independent incidence hashing.
- Maintain an XOR fingerprint of `(signature, multiplicity)` contributions.
  Equal rows/signatures do not cancel merely because their multiplicity is even.
  Fingerprint contributions update incrementally between snapshots, but graph
  export and incidence refinement still scan the entire snapshot. This is NOT yet
  the proposed event-fed, rule-effect-packet implementation.
- Key a standard HashMap by environment/obligation fingerprint and content
  fingerprint. Each key retains its first state; no r-representative selection,
  bucket scan or pair enumeration is performed. Approximate collisions can lose
  recall. Exact comparison and exact contract-string equality gate verified hits.
- Compare complete declared contents with the existing bounded isomorphism checker
  (10,000 search states; its existing graph-size limits apply). A hit reports a
  value mapping, not equivalent triggers or interchangeable native matcher state.
- Limit the table to 4,096 entries; capacity exhaustion loses observations, not
  correctness. Memory is O(total retained snapshot content), not O(number of hashes).
  Entry count bounds are not a byte budget. Exact-check budgets do not bound export.
- This observer uses whole isolated cells with no pending injections. It records
  sequentially generated snapshots; mutable visibility/table hits do not certify
  equivalent incremental scheduler state. No continuation is actually substituted.

The flat records are currently enode/table rows, not compiled rule-effect packets.
The first implementation deliberately reuses the existing verified exporter;
measurements show why the event-fed packet version is needed before enabling it
as an optimization. Sharing a suffix also needs explicit Tier1 history/port
remapping; silently skipping these records would invalidate downstream analysis.

## Results (local macOS, release, five alternating baseline/observer runs)

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
