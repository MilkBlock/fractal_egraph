# Deferred intermediate representation at the Run barrier

This is an opt-in experiment in the standalone, single-thread Cell CPU engine,
based on `0f3a4fda`. It uses the existing 64-unit iterative Bake programs. The
new proof comes from the target's typed instructions and actual pending unions;
it does not use recorded keeper IDs, workload names or an execution oracle.

## What is deferred

A constructor result used by a later table operation in the same RHS is an
intermediate. The compiler marks these positions in `deferred_incidence` and
the loader derives the certificate again. Executable Hide operations exclude
their entire target table. Initialization and repair-time construction keep
ordinary registration.

At an admitted birth, the engine examines only the unexamined suffix of the
pending union requests. If the canonical equality ID of a key appears as the
larger endpoint of one of those requests, that key must change at the next
repair. Representatives only move to smaller IDs. This is a sufficient guard;
it does not compute a speculative congruence closure or apply the union early.
Across a Run, prefix inspection is linear in the scanned requests, rather than
rescanning the whole prefix for each candidate birth.

The `edges` mode delays reverse dependency subscriptions. The `on` mode also
keeps the row in its existing instruction/slot representation, without a
decoded row payload at birth. An early keyed read can reconstruct the row.
Both modes retain the original logical ID, physical fact header, constructor
key directory entry, table membership and publication receipt. A missing key
still creates a logical fact; no consumer is allowed to miss that fact.

At repair, every deferred row enters the ordinary ordered dirty worklist and
its cached fields are invalidated before the queued unions run. The guard
ensures the ordinary engine would have dirtied the same row. A collision
retires it and forwards the same receipt; a survivor installs reverse edges
and a decoded payload through the original repair path. There are no deferred
records left at the next matching snapshot. Actual union and publication order
are unchanged.

## Scope of the result

On the frozen inputs, sum-bits defers 64 births and Raytrace defers 11,900. Of
these, 64 and 11,898 respectively retire before ever installing reverse
subscriptions. Raytrace's other two rows promote normally. The avoided birth
subscriptions contain 189 and 35,700 distinct equality dependencies.

Payload deferral avoids 64 / 11,900 birth row payloads and copying 189 / 35,700
values into those payloads. It incurs 15 / 1,134 row decodes before repair.
These counters describe specific operations, not RSS or cumulative Cell count.
The peak deferred sets contain 15 / 917 facts. Raytrace tests 186,850 eligible
births and scans 175,848 queued union pairs, so eligibility and guards are part
of the cost, not free work.

All 1,428 / 16,883 scheduled Runs, ordered RHS inputs, effective writes, final
graphs and Spine trajectories match the frozen Cell audit. Resumptions remain
3,034 / 663,403 and maximum chains remain 12 / 29. Forced fingerprint and Sieve
collisions also preserve the sum-bits audit. The source regression covers a
pre-repair read, a retired intermediate, a surviving intermediate, a later
scheduled consumer, missing union guards, a table with a later subsume, and a
forged certificate. These are within-Cell checks; they do not resolve native
egglog's separate extraction-choice comparison.

## Measurements

The unrestricted three-lane matrix used five measured rounds after one warm-up
per mode, rotating the order in fresh processes. It did not establish a net
speedup:

| Pass 3 | Inference ms, off / edges / on | CLI ms, off / edges / on |
|---|---:|---:|
| sum-bits | 18.293 / 18.228 / 18.232 | 112.544 / 113.726 / 111.990 |
| Raytrace | 6,777.669 / 6,789.922 / 6,792.511 | 10,118.511 / 10,080.250 / 10,173.754 |

All lanes retain 2,893 / 704,559 Cell registrations and 3,739 / 820,439 logical
fact creations. The feature stays off by default. Peak RSS is a separate
high-water measurement; fewer temporary allocations do not imply a lower peak.

The unrestricted matrix predates the addition of admission metadata and its
report count. The cross-transfer matrix below uses the final runtime. They
have separate binary fingerprints and independently paired controls; do not
subtract times between the matrices. No PGO or native-CPU override is used.

## Reproduce using Cargo

Prepare the frozen inputs with `cargo cell-compile --case sum-bits --output
out/sum-bits` and the corresponding `raytrace` command. Use new output
directories. For an ordinary inference benchmark:

```sh
cargo bench --locked -p eggfuse-cell-compiler \
  --features deferred-incidence,view-ports --bench cell -- \
  --program out/raytrace/program.json --deferred-incidence on --repeat 5
```

Use `off`, `edges` and `on` for the three lanes. For small fixtures,
`--reference-program FILE` verifies the initial graph and each Run's graph,
schedule and bindings against the ordinary mode before timing.

The paired driver starts fresh processes, rotates the three modes, excludes
one warm-up per mode, and checks the frozen graph and schedule on every sample.
It records inference, full CLI, peak RSS through inference, phase times, the
physical-work counters and binary/input hashes. CLI includes graph export;
builds and offline training are outside the measured processes.

```sh
cargo build --release --locked -p eggfuse-cell-runtime \
  --features deferred-incidence,view-ports,storage-metrics
cargo run --release --locked -p eggfuse-cell-runtime \
  --example bench_deferred_incidence \
  --features deferred-incidence,view-ports,storage-metrics -- \
  target/release/eggfuse-cell-runtime out/deferred-pairs \
  out/sum-bits/program.json out/raytrace/program.json 5
```

For separate ordered audits, add `slot-transfer-audit,spine-shapes` to both
builds and replace the final `5` with `0 --audit`. Diagnostic timings must not
be substituted into the uninstrumented matrix.

## Execution-guided admission

`bake_deferred_admission` consumes measured retirements and their training input
fingerprint. It selects the smallest descending-count prefix covering 80% of
retirements before reverse subscription. Names and complete table schemas make
the selection transferable. At load, this hint can only remove positions from
the independently derived typed certificate. A changed type is rejected, an
absent table admits no positions, and an empty hint uses ordinary registration.
This is materialization admission, not a new Cell-egraph equivalence proof.

```sh
cargo run --release --locked -p eggfuse-cell-runtime \
  --features deferred-incidence --example bake_deferred_admission -- \
  out/sum-bits/program.json out/deferred-pairs/summary.json sum-bits \
  out/raytrace/program.json out/selected-raytrace.json
```

Reverse the programs and use `raytrace` as the training case for the other
direction. The two independent profiles both selected `AddContext`; no source
code names this relation. On Raytrace, sum-bits admission reduces eligible
birth checks from 186,850 to 39,145, retaining 11,893 of 11,900 deferred births.
On sum-bits, Raytrace admission reduces checks from 706 to 197, retaining 61 of
64 births. The ordered audits still pass. The admission field changes the
artifact checksum, so the Spine audit compares the entire trajectory after
normalizing only its program-identity header to the artifact with that hint
removed; both raw and normalized hashes are saved.

The cross-transfer matrix also uses five measured rounds per mode after warm-up:

| Train → execute | Inference ms, off / edges / on | CLI ms, off / edges / on |
|---|---:|---:|
| Raytrace → sum-bits | 18.027 / 18.107 / 18.051 | 114.368 / 113.956 / 115.583 |
| sum-bits → Raytrace | 7,035.156 / 6,958.810 / 6,857.387 | 10,391.174 / 10,383.480 / 10,127.823 |

The full selected mode wins 2/5 sum-bits pairs and 4/5 Raytrace pairs. Median
paired inference differences are +0.024 ms and -109.171 ms. Raytrace's ratio
of medians improves by 2.53%; sum-bits has no measured benefit. Peak RSS medians
are 152.469 / 152.484 MiB and 1,850.719 / 1,850.922 MiB (off / on), providing
no evidence of a peak-memory reduction.

Raytrace's per-phase medians (off / on) are search 3,281.415 / 3,214.525 ms,
RHS 1,833.894 / 1,772.439 ms, repair 547.818 / 529.772 ms and query-index update
1,353.655 / 1,320.577 ms. Independent phase medians do not sum to the inference
median. Since unchanged search also moves, this workstation measurement does
not attribute the entire time difference to reverse-edge removal. The feature
remains opt-in; the evidence does not establish a generally faster default.

Raw repetitions, work counters, input hashes, audit digests and binary hashes
are preserved in [deferred-incidence/](deferred-incidence/). The final compatible
feature regression passes 16 compiler tests and 20 runtime tests, including the
existing memory-lifetime modes; a default-feature build is checked separately.

## Remaining block work

This experiment does not eliminate physical Cell registrations or logical fact
creation. It defers auxiliary representation for a provably transient subset;
global key entries and membership headers remain necessary for current readers.
The next representation change needs a block-owned lookup path and a complete
account of its readers before those headers can disappear. The already queued
union prefix supplies a useful guard, but does not justify replacing general
rebuild with a block-ID switch.

The workload counters rank actual avoided storage work, including guard misses
and early decoding. The admission profiles were trained from those counters;
no extra cross-Run combinations were trained for this experiment.
Increasing the 64-unit catalog or structural depth alone is not evidence that
more global physical work can be omitted.
