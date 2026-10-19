# Commit-driven Cell interfaces and boundary queries

The shared GPU/native loop now has an **interface state machine** built from
analyze's certified positive Cell projections. Native row commits advance these
states and publish ready ports. This is not a claim that an entire arbitrary
RipSatCell's graft/RHS has been compiled away. The original RHS, merge, union and
rebuild remain authoritative in egglog.

## Runtime path

```text
native committed row additions / updates / deletions / subsumption
    -> GPU keyed reverse watches (including missing keys)
    -> dirty interface queue, earliest affected step
    -> retained prefix binding + actual committed row + necessary suffix reads
    -> Waiting / Ready / Uncertified / Dormant
    -> ready-port delta + external residual query
    -> original native RHS and rebuild
    -> the next committed delta
```

A projection whose anchor is new is initialized once. A missing fact leaves its
partial binding and exact missing key registered. A later commit can provide that
row directly; it is checked against the current key, type and binding before use.
Changing a later witness preserves earlier valid row references, so they are
rebound directly without another index lookup. A changed prefix can change the
required suffix keys; the old watch revision expires and new keys are registered.
Failed alias/literal checks keep the ordinary query available.

The CPU transports committed rows and skips batches whose old/new **table tags**
are absent from the compiled Cell dependency set. It does not maintain a Cell
instance registry, inspect match history, or solve bindings. Concrete keyed
wakes, state updates, waiting keys, projection binding frames and ready ports
reside on the GPU. GPU-generated indirect dispatch sizes execute only the dirty
queue; changed batches no longer scan every row/projection certificate.

## Two disjoint query lanes

Each eligible consumer gets one driving relation: its largest witnessed
functional fragment (stable source-order ties). All remaining LHS atoms and
guards stay in its residual query. This structural choice is not a calibrated
cost optimizer, and it does not cap candidate matches.

* If any external atom is new, the ordinary external-delta lane owns the match.
  It can still reuse the internal projection when entering the Cell.
* If only internal atoms are new and the interface is Ready, its port drives the
  residual query; every external row must be older than the rule's watermark.
* If the interface is unrecognized, waiting, or invalid, ordinary matching owns
  it. No second full query runs for a binding already delegated to a ready port.

All external atoms rank before the internal fragment for delta ownership. A
cached segment that jumps over the port's anchor must delegate too: omitting this
case would submit the same match twice. Original per-rule watermarks and sorted
RHS commit boundaries are preserved. Complete native frontier audits remain
independent of the GPU's choice of ownership.

Port revisions remove obsolete bindings after union, row updates, or slot reuse.
Watch and port logs compact by live revisions before their geometrically grown
capacity is exhausted. Late consumers retain their own cursors; compaction resets
cursors but native timestamps prevent historical matches from replaying. There
is no match truncation or permanent completed-match set.

## Tests

`tests/cell_transitions.rs` tests an actual native helper RHS that completes a
waiting Cell, direct port matching against an old boundary, numerical guards,
resuming a missing suffix from retained bindings, and hundreds of delete/reinsert
updates while another consumer is delayed. The stateful merge counter verifies
that compaction neither loses nor replays an application. Existing tests cover
new external facts, unknown shapes, alias-changing union, subsume, two Cell
segments, slot recycling, and native schedule handoff/handback.

```sh
mkdir -p out/cell-event-worker
xcrun swiftc -O cell-assisted-egglog/matrix.swift -o out/cell-event-worker/matrix
CELL_MATRIX_WORKER="$PWD/out/cell-event-worker/matrix" RAYON_NUM_THREADS=4 \
  cargo test --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted --test cell_transitions --test cell_prefix --test table_live
```

Live worker protocol is **10**; rebuild old workers. Offline matrix ABI is still 5.

## Ablation

Reuse the existing runner and frozen If analyze library, with separate timing,
audit and profiling runs. Each lane executes once; no average is taken.

```sh
python3 cell-assisted-egglog/cell-prefix-bench/run.py \
  --analysis out/cell-resident-verified/eggcc-if-sparse/analyze \
  --worker out/cell-event-worker/matrix --output out/cell-events-time
python3 cell-assisted-egglog/cell-prefix-bench/run.py --skip-build \
  --analysis out/cell-resident-verified/eggcc-if-sparse/analyze \
  --worker out/cell-event-worker/matrix --output out/cell-events-check --mode check
python3 cell-assisted-egglog/cell-prefix-bench/run.py --skip-build \
  --analysis out/cell-resident-verified/eggcc-if-sparse/analyze \
  --worker out/cell-event-worker/matrix --output out/cell-events-profile --mode profile
```

The controls are native cached queries with the same commit policy, full GPU,
Cell-guided order only, the previous cached Cell path, and the new event path.
The latter is named `cell_certificates` in JSON; `cell_cached` isolates the old
maintenance and query behavior. Cumulative profiling counters include ordinary
`join_probes`, `port_probes`, `certification_probes`, `dirty_cells`, retained
`prefix_rows_reused`, `direct_row_reuses`, `port_bindings`, and delegated branches.
Direct row reuse includes both unchanged carried witnesses and newly committed
row hints. Logical port bindings are not necessarily committed mutations.

## Explicit limits

This advances **Cell interface states**, not arbitrary catalog-state transitions
or full replacement of a coarse rule's RHS. Initialization can still need keyed
reads. Multi-result reverse projections and unknown specializations retain full
queries. The selected port need not be the most profitable port for a workload.
In particular, a ready port created at bootstrap can receive no subsequent
internal changes; matches may then be entirely external-delta driven.

Certificates now use [sparse observed-instance storage](../../experiments/sparse_cell_storage/README.md).
Reverse watches and ports are sized from reached keys and actual Ready states;
they still cost memory. Normal wake work depends on changed rows, matching keyed
subscribers and affected suffixes. Static per-table lists limit anchor discovery
to relevant plans; per-row interface lookup and hash collisions add checks.
Compaction scans retained logs and exact sizing adds synchronization. Setup,
device memory and all runtime overhead must remain in the reports. Earlier
results in this directory used the historical dense reservation layout.
