# Materializing a Spine root at its first published anchor

The parent executor (`56f107f`) allocates an `Rc<RefCell<Context>>` for each fresh producer binding at RHS entry. The new executor reserves the same logical ID and source position at that point. Completed anchor-producing applications occupy a slot in a reusable Run-local vector; every anchor of one application points to the same slot. Publication still canonicalizes keys after repair, keeps the last producer for each key, follows same-key forwarding, and checks receipt visibility or performs the exact lookup. The first successful installation promotes the slot to a full cursor. Other winning anchors from that application clone the same cursor. Resumed applications carry their existing context through the same boundary.

The runtime counts logical roots in `root_attempts` and counts the full root allocation at the actual allocation site in `contexts_created`. Both counters are exposed by the Cargo benchmark module. In the eager control, each root attempt always allocated a full context. Explicit callers of the existing public `Programs::begin` API still receive a materialized cursor immediately. The normal executor uses the deferred path without an additional configuration switch.

## Frozen Pass 3 ablation

| Input | Eager full roots | Deferred full roots | Allocations avoided | Inference median, eager → deferred | Faster pairs |
|---|---:|---:|---:|---:|---:|
| sum-bits | 4,891 | 3,872 | 1,019 (20.83%) | 81.36 → 78.61 ms | 5/7 |
| raytrace | 2,481,357 | 1,488,359 | 992,998 (40.02%) | 36.79 → 35.56 s | 2/3 |

The expected savings from the previous distinct-root census were 1,019 on sum-bits and 992,998 on Raytrace. This experiment counts the full allocations directly. Raytrace still materializes the 1,015,453 roots that did publish but never resumed in that census. The ticket, pending-vector storage, and anchor keys still have costs. These counts give neither a process-wide allocation count nor peak RSS.

| Work verified equal across the eager and deferred lanes | sum-bits | Raytrace |
|---|---:|---:|
| Logical root attempts | 4,891 | 2,481,357 |
| Resumptions | 3,034 | 663,403 |
| Maximum chain depth | 12 | 29 |
| End-of-schedule live contexts reachable from anchors | 1,281 | 268,621 |

Both lanes run the same frozen program in the standalone Cell CPU executor, using planned Free Join, shared nodes, Sieve, column intersection, stage Spine and anchor receipts. Each timed sample starts a fresh process. After an excluded warm-up for each lane, order alternates each round (seven rounds on sum-bits, three on Raytrace). The inference timer excludes compilation, setup, export and audit. Every measured sample matches its original full graph endpoint and per-Run schedule digest, including logical and executed binding counts.

In separate untimed builds of each revision, SHA-256 is fed the ordered `(rule ID, slot/value map)` records of the prepared RHS patch before application. Each Run has its own digest, including empty Runs. Compare both arrays in original order. These runs use the source schedule with local saturation disabled. All 1,428 and 16,883 elements respectively match. The stored checksum of each entire digest array is also provided. An additional sum-bits Cargo audit checks graph, binding and schedule snapshots against Spine off.

## Reproduce

```sh
cargo cell-compile --case sum-bits --emit-free-join-entry --output out/root-sum
cargo cell-compile --case raytrace --emit-free-join-entry --output out/root-ray
cargo cell-bench --program out/root-sum/program.json --lane free-join-planned --modules stage-spine --repeat 7 --verify-stages
cargo cell-bench --program out/root-ray/program.json --lane free-join-planned --modules stage-spine --repeat 3
```

Use the same artifacts and options in a second checkout at `56f107f` for eager roots. Use `--repeat 1` with alternating fresh processes to reproduce the timing protocol above. For the RHS check, apply [the audit patch](root-rhs-audit.patch) with `git apply --unidiff-zero` in a temporary copy of each checkout. Run the following for each compiled input at both revisions, with new output directories, and compare the two `rhs.json` files using `cmp`.

```sh
CHECK_RHS_DIGESTS=1 CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 \
CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1 \
CELL_CPU_FREE_JOIN=planned cargo cell-run --program out/root-ray/program.json --output out/ray-rhs
```

[Raw samples, phase medians, work counters and per-Run audit fingerprints](root-results.json)
