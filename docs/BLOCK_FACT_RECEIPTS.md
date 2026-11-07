# Exact fact references for physical block ports

This opt-in standalone Cell CPU change extends `4be8fd7a`. It addresses the
27,958 Raytrace attempts where a physical block had an imported result identity
but could not locate the constructor fact that produced it. The result identity
is an e-class value, not a physical fact address.

## Execution and lifetime

`--block-receipts` enables the physical block LHS route and supplies original
constructor receipts to it. The existing RHS receipt array is prepared before
the constructor-only prefix. A successful reuse, construction, constructor set,
or existing handoff writes its actual dependency into that array. Statically
skipped operations resolve through their compiler-checked query/effect aliases.
No fallback key lookup is performed merely to capture a receipt.

After RHS execution, the engine saves only receipts for a physical block that
survived. Reused-only temporary frames leave no persistent receipt record.
Self-owned facts already have addresses and need no additional entry. The common
single-receipt record is inline; larger records remain sorted by instruction
position. Logical Cell IDs make records independent of compact-slab relocation.
Retirement removes the source block's metadata, including detached children
that are discarded. The receipts do not pin their target facts or create Cells.

At a consumer's original scheduled entry, a stored receipt is resolved through
the existing forwarding mechanism. The engine checks the current table, complete
canonical key and visibility. Deleted, hidden, unavailable or mismatched facts
fall back. Existing bound-result constraints, epochs, residual guards and ordered
RHS execution remain active.

The compiler's normalized postconditions and complete-key plans constrain what
is retained. A same-root symbolic plan must identify a usable non-nullary port;
producers with checked outgoing multi-Run edges retain conservative demand for
later shared layouts. Other unadmitted shapes continue through their original
fallbacks. This analysis runs at configuration time; it uses types and dataflow,
not rule names or workload-specific conditions.

## The nullary finding

The first direct-reference audit served 24,863 reads; 24,801 had empty keys.
The ordinary engine already has a singleton fast path for these tables. They
were counted as global key-lookup calls, but did not compute a key hash or
search a hash bucket.

The final implementation uses the table directory's existing exact fact
reference when the planned nullary table has one member. It validates the actual
row and its visibility. It does not retain another copy per Cell or introduce a
new constant cache. This also allows a deleted and recreated nullary fact to be
read at its current identity; original result equality checks still apply.

Non-nullary receipts remain per surviving block. The initial broad collection
ended Raytrace with 109,322 references. Removing nullary copies and narrowing
the capture plans reduced this to 12,010 with the same observed coverage.
Inlining the common one-reference record reduced payload capacity to 144,120
bytes. This excludes the source map and record headers and is not peak RSS.

## Isolated results on the original 64-unit programs

Both arms enable multi-Run ports and physical block LHS matching. Only exact
receipt delivery and the shared nullary port differ. The iterative LHS catalogs
are absent in this comparison.

| Metric | sum-bits old / new | Raytrace old / new |
|---|---:|---:|
| Imported-identity hint failures | 84 / 0 | 27,958 / 0 |
| All global lookup calls | 19,344 / 19,300 | 4,420,979 / 4,396,218 |
| Global singleton fast-path calls | 596 / 554 | 164,790 / 140,036 |
| Fact-table hash probes | 17,496 / 17,494 | 4,128,092 / 4,128,085 |
| Served physical block atoms | 8 / 52 | 2,107 / 26,923 |
| New non-nullary receipt reads | 0 / 2 | 0 / 62 |
| Shared nullary-directory reads | 0 / 42 | 0 / 25,686 |

Raytrace saves 24,761 global calls, of which 24,754 are singleton fast-path
calls. Actual hash-based fact-table probes fall by only seven. This latter
counter includes constructors and virtual relations; function-key indexing is
separate. The two split counters exist only in diagnostic builds, so they add
no instrumentation to the timed binary.

The served-atom increase is 24,816, which is not identical to the net global-call
reduction. The latter also reflects the effect of actual receipts on the
remaining execution/publication paths. No sum of local counters is substituted
for the directly measured global delta.

The ordinary release build, single-threaded on Apple M3 Max with rustc 1.94.0
and no PGO, uses five fresh-process pairs after an excluded warm-up. Mode order
alternates; training and builds are excluded. Diagnostic timings are not used.

| Workload | Inference old / new, ms | CLI old / new, ms | Peak RSS old / new, MiB |
|---|---:|---:|---:|
| sum-bits | 23.500 / 23.495 | 130.253 / 134.070 | 152.922 / 152.984 |
| Raytrace | 8,343.783 / 8,350.680 | 11,390.602 / 11,442.458 | 1,826.688 / 1,828.016 |

This is effectively unchanged inference time, with added setup/CLI cost. Only
two of five Raytrace pairs improve. The capability stays **off by default**.
The new capture analysis costs approximately 29 ms on the diagnostic Raytrace
run, outside inference, and the nested capture timer is approximately 2.6 ms.
These nested timers exclude some allocation and destruction costs; whole-run
timing is authoritative.

## Combination with iterative Bake

The combination matrix uses the selected, unchanged learned artifacts from the
[iterative LHS study](ITERATIVE_LHS_BAKE.md): sum-bits round one and Raytrace
round two. All four modes use those same artifacts and RHS programs:

* `none`: physical block matching and learned LHS reads disabled.
* `receipts`: physical blocks with exact receipts; learned LHS reads disabled.
* `iterative`: learned LHS reads only.
* `combined`: both enabled.

The ordinary Spine context cache runs first, then learned receipts, then the
physical block route, followed by ordinary lookup. The earlier branch already
provided this routing order; the change fills the physical route's missing
fact-reference source. There is no second copy of a logical graph row. The two
metadata lifetimes remain distinct: bounded continuation history and surviving
physical blocks.

Aggregate overlap is reported as the sum of individual global-call savings minus
the combined saving. This is a net work interaction, not a claim that runtime
events have been paired by identity. Function reads supported by iterative Bake
remain outside the constructor-only physical block receipt contract.

| Workload / mode | Global calls | Singleton fast calls | Fact-table hash probes | Learned LHS hits | Physical LHS hits |
|---|---:|---:|---:|---:|---:|
| sum-bits / none | 19,347 | 596 | 17,499 | 0 | 0 |
| sum-bits / receipts | 19,295 | 554 | 17,489 | 0 | 52 |
| sum-bits / iterative | 19,331 | 592 | 17,492 | 16 | 0 |
| sum-bits / combined | 19,283 | 554 | 17,482 | 16 | 48 |
| Raytrace / none | 4,423,069 | 165,722 | 4,129,250 | 0 | 0 |
| Raytrace / receipts | 4,396,201 | 140,036 | 4,128,068 | 0 | 26,923 |
| Raytrace / iterative | 4,422,259 | 165,567 | 4,128,761 | 810 | 0 |
| Raytrace / combined | 4,395,546 | 140,036 | 4,127,579 | 810 | 26,768 |

The combined Raytrace mode saves 27,523 calls versus `none`. The net overlap is
155, entirely in singleton fast calls. Adding iterative LHS to the receipt mode
saves another 655 calls: 489 fact-table hash probes and 166 function-key reads.
sum-bits has four overlapping singleton calls and twelve additional reads saved
by the combination. The two optimizations therefore complement each other in
the measured keyed work, while overlapping on cheap nullary reads.

The `receipts` arm includes the previous physical block matcher as well as this
change. Its 26,868-call Raytrace reduction must not all be attributed to new
receipt storage; the isolated 64-unit comparison above measures that increment.

Three fresh-process measurements per mode follow an excluded warm-up; the order
rotates across four modes. The diagnostic counters are absent from this binary.

| Workload / mode | Inference ms | Complete CLI ms | Peak RSS MiB |
|---|---:|---:|---:|
| sum-bits / none | 24.310 | 133.164 | 153.875 |
| sum-bits / receipts | 24.634 | 138.930 | 154.031 |
| sum-bits / iterative | 22.967 | 134.186 | 153.844 |
| sum-bits / combined | 23.062 | 134.019 | 154.031 |
| Raytrace / none | 8,464.833 | 11,468.237 | 1,801.141 |
| Raytrace / receipts | 8,447.784 | 11,516.759 | 1,821.219 |
| Raytrace / iterative | 8,408.695 | 11,458.737 | 1,816.359 |
| Raytrace / combined | 8,563.161 | 11,636.345 | 1,828.422 |

The combination establishes complementary saved work, not an overall speedup.
Raytrace's combined median is 1.16% slower than `none` in this small matrix.
sum-bits' inference improvement is already present in the iterative-only arm;
adding the physical receipt route does not improve that median. These results
do not justify enabling either experimental route by default.

## Validation and limits

The regression matrix covers imported identities, ordinary reuse, static query
and effect aliases, nullary and keyed constructors, deletion, hiding,
delete/recreate, union, and source retirement. Every fixture compares each Run's
graph and RHS bindings with its control. New constant ports also verify that
bound-result constraints survive replacement of the referenced nullary fact.

The Cargo tests pass 28 compiler and 29 runtime tests, including structural/RHS
audit and local-RHS features. The default build checks separately. Workload
audits compare the frozen full state, schedule, ordered RHS inputs, effective
writes, resume stream and Spine shapes. Only the artifact-identity header of
the shape stream is normalized for learned catalogs.
The combined sum-bits forced hash/Sieve-collision run also matches the frozen
digests. Resumptions/longest chains remain 3,034/12 and 663,403/29. Full Cell
registrations remain 968 and 319,326; this change does not eliminate Cell births.

These are standalone Cell-versus-Cell checks. They do not establish native
egglog equivalence. Fewer global lookup calls, especially singleton calls, do
not establish a speedup or a reduction in logical nodes.

[Audit results](block-fact-receipts/audits.json),
[timing summaries](block-fact-receipts/results.json), and raw isolated/combination
JSONL samples are saved in `docs/block-fact-receipts`. The proof/capture changes
use the same frozen artifacts and existing iterative catalogs; no new Bake
training or workload-specific rule is introduced in this experiment.

## Cargo reproduction

Use fresh output directories and the frozen multi-Run inputs documented in
[MULTI_RUN_PORTS.md](MULTI_RUN_PORTS.md#cargo-reproduction). The combination
programs are the selected artifacts from [iterative Bake](ITERATIVE_LHS_BAKE.md#cargo-reproduction).

```sh
cargo build --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks,view-ports,storage-metrics \
  --bin eggfuse-cell-runtime --example bench_deferred_incidence

# Original 64-unit inputs: isolate exact-reference delivery.
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/receipt-pairs \
  out/multi-sum.json out/multi-ray.json 5 --block-receipts

# Identical learned catalogs in four modes.
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/receipt-combinations \
  out/lhs-sum/program.json out/lhs-ray/program.json 3 --receipt-combination \
  out/multi-sum.json out/multi-ray.json

# Add slot-transfer-audit,spine-shapes to the runtime build. Use zero rounds
# and --audit-block-receipts / --audit-receipt-combination for full audits.

cargo run --release --locked -p eggfuse-cell-compiler \
  --features two-run-blocks,view-ports -- bench \
  --program out/fixture/program.json --reference-program out/fixture/program.json \
  --multi-run-blocks on --block-receipts on --repeat 1
```
