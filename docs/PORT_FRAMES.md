# Stable facts backed by compact block ports

This opt-in standalone Cell CPU experiment builds on `c4f48685`. It separates
the logical producer identity from its full Cell record. The frozen 64-unit
Bake catalogs, rule matching, RHS effects and Run schedule are unchanged.

## Representation and admission

The compiler admits completed blocks containing copies, unions and at most four
non-eliminated virtual-constructor operations. Static constructor aliases are
checked first. Mutable function writes, arbitrary reads, primitives and Hide
operations keep the ordinary path. The emitted `port_frames` certificate is
derived again by the loader; a forged instruction position is rejected.

During execution a candidate frame remains temporary through its RHS. When it
completes, parameters and fact states move into arrays shared by the template,
stored-slot layout and present fact positions. Input-slot IDs and instruction
positions are recorded once per layout, rather than in a separate map for each
producer. Existing logical Cell IDs are still reserved in exactly the same
order. A FactRef therefore keeps the same `(logical Cell ID, instruction)`.

```text
FactRef(logical ID, instruction)
    -> shared logical-ID directory
    -> full Cell, or (layout, row)
    -> packed parameters and the requested fact port
```

The high directory bit distinguishes full Cells from compact rows. The other
bits encode up to 512 layouts and 4,194,304 rows per layout. Exceeding either
limit falls back to a full Cell before installing any compact state. Removing
a compact row relocates at most one row and updates its directory entry; no
logical references change. The directory belongs to both representations and
must not be counted twice in memory totals.

Consumers use the same canonical-key, visibility and epoch checks for either
representation. Spine publication also uses that common fact-state accessor.
Repair, subsumption and deletion update the original logical fact. A retained
parent keeps its immutable parameters even after its fact is retired; a child
port can read those parameters until its last reference is released. Parent
columns have a separate packed column only in layouts that actually use them.
The two measured workloads have zero retained borrowed-parent records at the
endpoint, so parent support is covered by tests, not credited as a measured
parameter-sharing gain.

## Whole-frame census

The Raytrace census observes 496,886 registered RHS frames that own constructor
facts. Initialization and merge-generated frames are outside this count. Of
these, 348,186 have one fact left after repair, and 21,308 become empty and
unretained. No captured frame has all its facts covered by the previous
deferred-incidence certificate: an eligible intermediate often shares its Cell
with another output. Thus 11,900 deferred facts did not imply 11,900 deletable
whole Cells. The compact-port path instead handles complete certified blocks.

## Measurements

Each matrix uses five alternating fresh-process pairs after one excluded
warm-up per mode. Both sides are ordinary release builds on one Apple M3 Max
CPU thread, without PGO. Build and offline training are outside timing. CLI
includes loading and full graph export; RSS is the high-water through inference.

The first prototype used an additional hash directory for compact producers:

| Workload | Inference ms, off / on | CLI ms, off / on | Peak MiB, off / on |
|---|---:|---:|---:|
| sum-bits | 18.978 / 19.491 | 121.290 / 124.410 | 152.516 / 152.234 |
| Raytrace | 6,987.871 / 7,554.755 | 13,415.092 / 13,205.295 | 1,853.750 / 1,822.906 |

The final implementation reuses the ordinary logical-ID directory:

| Workload | Inference ms, off / on | CLI ms, off / on | Peak MiB, off / on |
|---|---:|---:|---:|
| sum-bits | 18.502 / 18.156 | 115.806 / 114.531 | 152.516 / 152.234 |
| Raytrace | 6,862.728 / 6,710.549 | 12,703.221 / 12,425.725 | 1,868.844 / 1,806.813 |

Raytrace is faster in all five final pairs, with a median paired difference of
-125.006 ms and a 2.22% reduction in the ratio of inference medians. Peak RSS
falls by 62.031 MiB (3.32%). Sum-bits has only two faster pairs and a median
paired difference of +0.066 ms; its lower ratio of medians does not establish a
repeatable gain. The workstation is not isolated. The two matrices have separate
controls and executable hashes; their absolute times must not be subtracted to
attribute the directory change. The hash prototype is retained as historical
measurement evidence, not as another runtime option.

| Representation metric | sum-bits, off → on | Raytrace, off → on |
|---|---:|---:|
| Full Cell registrations | 2,893 → 968 | 704,559 → 319,326 |
| Compact records installed | 0 → 1,925 | 0 → 385,233 |
| Full Cells live at finish | 2,385 → 953 | 601,924 → 300,469 |
| Compact records live at finish | 0 → 1,432 | 0 → 301,455 |
| Full + compact + embedded writer records | 2,875 → 2,875 | 687,913 → 687,913 |
| Logical facts created | 3,739 → 3,739 | 820,439 → 820,439 |

This is a representation compression, not deletion of logical producer
instances. Temporary RHS frames still exist before sealing; the experiment does
not claim to eliminate every allocation at RHS entry. The final compact stores
use 156 / 246 shared layouts. There are 404 / 51,641 producer-context requests
through ports born in earlier Runs; these are context requests, not an additional
count of successful Spine resumptions.

## Correctness and remaining work

Both workloads match the frozen graph, schedule, ordered RHS inputs, effective
writes and complete Spine trajectory. Resumptions remain 3,034 / 663,403, with
maximum chains 12 / 29. All ordinary work counters agree except full Cell
registration. The sum-bits audit also passes with forced fingerprint and Sieve
collisions. Enabling the previous deferred-incidence path together with ports
also preserves both workloads' five graph/ordered-audit digests; this combined
diagnostic is not a performance measurement. Tests cover cross-Run consumption, union repair, subsumption,
retained deleted parents, slab relocation, mixed full/port directory entries,
bounded-layout fallback and certificate tampering. Default-feature builds and
the optional memory-lifetime features are checked separately.

The path stays off by default. It does not fuse A and B into a single logical
producer or suppress global indexes/publications. It establishes the port-backed
identity and lifetime interface needed for that next change. Private intermediate
facts need a complete reader contract and discoverability mechanism before their
global entries can disappear. This study makes no new claim about native egglog
equivalence or a twofold end-to-end improvement.

## Cargo reproduction

Prepare the usual frozen inputs with `cargo cell-compile --case sum-bits --output
out/sum-bits` and the corresponding Raytrace command. Use fresh output directories.

```sh
cargo build --release --locked -p eggfuse-cell-runtime \
  --features port-frames,view-ports,storage-metrics
cargo run --release --locked -p eggfuse-cell-runtime \
  --example bench_deferred_incidence \
  --features port-frames,view-ports,storage-metrics -- \
  target/release/eggfuse-cell-runtime out/ports-paired \
  out/sum-bits/program.json out/raytrace/program.json 5 --ports
```

The driver disables deferred incidence in both arms and changes only
`--port-frames`. For separate ordered audits, add
`slot-transfer-audit,spine-shapes,receipt-slots` to both builds and replace
`5 --ports` with `0 --audit-ports`. `--frame-census` on the runtime collects the
whole-frame ledger; use it with `--deferred-incidence on` and without
`--port-frames` to reproduce the earlier opportunity census. Diagnostic times
are excluded from the performance matrices.

New compiler artifacts can emit the certificate with
`cargo run --release --locked -p eggfuse-cell-compiler --features port-frames -- compile ...`.
The measured frozen artifacts predate that field, so the loader derives their
checked plan during setup. [Raw measurements and hashes](port-frames/).

The ordinary compiler bench also accepts `--port-frames on|off` when built with
the feature. For example, `cargo bench --locked -p eggfuse-cell-compiler
--features port-frames,view-ports --bench cell -- --program FILE
--port-frames on --repeat 5`. Small fixtures can add `--reference-program FILE`
to compare every Run against the ordinary representation before timing.
