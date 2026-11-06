# Iterative Bake for cross-Run LHS completion

This opt-in standalone Cell CPU experiment extends `bfda344d`. A learned block
can supply consumer LHS rows from earlier scheduled executions, including actual
function rows. It uses the existing iterative catalog, state machine and bounded
receipt history. It does not require the stages to occupy one physical shared
parameter slab, which limited the preceding whole-block LHS experiment.

## Learning and execution

The old 64-unit catalog has 32 base entries selected for RHS reuse and 32
compositions. That seed omits valid residual consumer entries. The new objective
admits the program's checked stage entries as additional leaves: these two
programs each have 256 entries, so the warm catalog starts with 288 units.
Existing compositions are preserved with remapped IDs. A leaf without a compact
single-port instruction block continues through the original ordered matcher.
Admission never bypasses an unproved LHS atom or creates a new schedule entry.

Each round starts from the identical original input. Applied adjacent units
provide join evidence; the training path separately counts **actual positive
global key probes** by candidate join and query atom. The compiler symbolically
composes the path's ports and proposes only reads whose complete keys match an
earlier query or committed effect. A new composition is ranked by additional
proved reads that were still globally probed, subtracting the current catalog's
inherited coverage. This avoids rewarding reads already served by the existing
Spine cache. These scores are estimates, not successful port hits.

A bridge `A+B` may be admitted when observed `B+C` and compiler analysis show
that `A+B+C` can reuse a fact from `A`. The next round must actually observe the
learned `A+B` followed by `C` before admitting that larger composition. Both
operands may themselves be previously learned blocks. The regression tests
exercise this multi-round growth rather than directly installing a triple.

For this experiment the maximum expanded path depth is four, node budget eight,
total catalog budget 384, at most eight additions per round, candidate-counter
budget 2,048, and minimum support two. Depth is checked on expanded paths, not
just the number of binary DAG nodes. Each round exports a bounded Cell e-graph
with the composition parents and its checked terminal descriptor. This is the
existing finite CPU-block registry; no unbounded new RipSat saturation is run.

The selected checkpoint minimizes actual global key probes, then captured
receipts, then catalog size. Training wall time is excluded from that decision.
An unhelpful final round therefore cannot automatically replace a better earlier
checkpoint. This is a work-count objective; it does not certify faster execution.

## What the compiler proves and the engine checks

The serialized `lhs_handoffs` state machine is independently rebuilt on load.
Each read names a query atom, source rule, query/effect port and backward stage
distance. Constructor equality can propagate through keys and earlier result
ports. A mutable function's historical value does not unify later result
symbols or become a constructor RHS-completion certificate.

At the original consumer Run, the existing Spine cache gets first chance. On a
cache miss the new plan reads its retained dependency and validates the actual
operator, full canonical key and visibility/liveness. The returned row supplies
its **current value and epoch**. Original result constraints, primitive guards,
seminaive checks, answer order and RHS ordering remain active. A missing or stale
reference falls back to the ordinary lookup; it is never negative evidence.

Function references identify the committed table row, not a past RHS proposal.
An intervening merge can change that row's value; the consumer sees the merged
value. Deletion or a key-changing union can invalidate the reference and trigger
fallback. No unchanged-value or closed mutable-RHS claim is introduced.

Query witnesses can now carry either constructor or function dependencies.
History references retain their original eight-byte representation: nonzero
owner IDs encode constructor facts, and the unused zero owner encodes a bounded
function-row index. An unencodable index is omitted and falls back. No graph
fact is pinned merely by adding a history reference. Existing history lifetime
and maximum-back-distance limits apply.

## Metrics

| Question | Measurement |
|---|---|
| Did training see work left to remove? | Positive global probes by candidate join/atom; `measured_lhs_probes` and predicted additional reads |
| Did the new route really execute? | `attempts`, `hits`, missing receipts and guard fallbacks after the ordinary context-cache miss |
| Is it cross-stage and how deep? | `by_back`; distance two means a reference retained through an intervening stage |
| Which block served it? | `by_unit`, joined with that catalog's expanded path |
| Did functions participate? | `function_hits`, using current committed rows |
| What did retention cost? | Total captured receipts, history slots copied and nested capture time, compared with LHS disabled on the identical catalog |
| Did physical work fall? | Global key probes, constructor requests, full Cell registrations and logical fact creation separately |
| Did it run faster or use less memory? | Fresh-process inference/CLI medians and peak RSS, outside training and diagnostic builds |
| Did it generalize? | Rule-signature-based catalog transfer to the other workload, followed by independent proof reconstruction and full execution audits |

`by_unit` assigns each actual served read once to the active compiled unit,
including inherited suffix reads. It is not a leave-one-unit-out causal estimate.
The selection score separately subtracts inherited coverage. Captured receipts
and copied history slots count all existing RHS and new LHS history work; their
off/on difference isolates the added retention. Nested timers do not include all
dispatch, allocation or destruction costs.

## Learning results

The frozen source programs, initial graphs and original 64-unit catalogs are the
same as the preceding experiment. The additional entries are selected by typed
stage eligibility, not workload names or literal input values.

| Training workload | Round | Units | Actual LHS hits | Function hits | Global key probes |
|---|---:|---:|---:|---:|---:|
| sum-bits | 0 | 288 | 0 | 0 | 19,352 |
| sum-bits | 1, selected | 296 | 16 | 5 | 19,331 |
| Raytrace | 0 | 288 | 139 | 0 | 4,422,947 |
| Raytrace | 1 | 296 | 797 | 164 | 4,422,281 |
| Raytrace | 2, selected | 304 | 810 | 166 | 4,422,259 |
| Raytrace | 3 | 312 | 810 | 166 | 4,422,259 |

Raytrace's served LHS reads increase 5.83 times over round zero; 53 selected-round
hits reach back two stages. sum-bits stops after the first growth round. Ranking
only theoretical reuse had retained seven extra sum-bits compositions without
adding hits; the actual-probe ranking avoids those additions.

On the selected **identical catalog**, disabling only LHS ports gives:

| Metric | sum-bits off / on | Raytrace off / on |
|---|---:|---:|
| Global key probes | 19,347 / 19,331 | 4,423,069 / 4,422,259 |
| LHS attempts / hits when enabled | 18 / 16 | 855 / 810 |
| Total retained receipt captures | 1,530 / 1,818 | 159,890 / 191,393 |
| Total historical slots copied | 0 / 0 | 0 / 79,890 |

The LHS mechanism alone saves exactly 16 / 810 probes. Other probe reductions
relative to the old 64-unit catalog come from RHS reuse in the larger catalog.
Raytrace adds 31,503 receipt captures for 810 successful reads, about 38.9 added
captures per hit. Its diagnostic nested capture timer rises from 13.436 to
19.843 ms, while the new read guards themselves take 0.110 ms. These measurements
explain why better hit coverage need not yield a speedup. Absolute saved search
work remains a small part of the workload.

The two percentages have different denominators: guard success is 810/855
(94.7%), while overall coverage is only 810/1,574,999 uncached Spine port
refreshes (0.0514%). The 5.83-fold increase is a count of served reads, not a
5.83-fold improvement in guard success or total inference speed.

## End-to-end measurements and decision

Both timing matrices use one ordinary release binary, Apple M3 Max, rustc 1.94.0,
one CPU thread and no PGO. Five fresh-process pairs follow an excluded warm-up;
mode order alternates. CLI includes loading and export. Training, builds and
diagnostic runs are excluded. The physical whole-block `--block-lhs` experiment
is disabled in both arms; compact ports and multi-Run completion remain enabled.

| Comparison / workload | Inference off / on, ms | CLI off / on, ms | Peak RSS off / on, MiB |
|---|---:|---:|---:|
| Same learned catalog, sum-bits | 22.829 / 22.748 | 132.618 / 133.165 | 153.859 / 153.813 |
| Same learned catalog, Raytrace | 8,272.116 / 8,488.770 | 12,079.124 / 12,311.903 | 1,800.828 / 1,810.703 |
| Old 64 units vs transferred catalog, sum-bits | 23.052 / 23.077 | 130.604 / 136.905 | 152.797 / 169.734 |
| Old 64 units vs transferred catalog, Raytrace | 8,487.348 / 8,352.628 | 12,332.328 / 12,177.758 | 1,807.625 / 1,798.938 |

In the isolated comparison Raytrace is 2.62% slower by medians and slower in four
of five pairs, with 9.875 MiB more peak RSS. sum-bits is effectively unchanged.
The transferred Raytrace catalog is 1.59% faster by medians, but only three pairs
improve and pair differences range from -368.294 to +258.591 ms. This does not
establish a reliable speedup. RSS includes validation IR and allocator capacity,
not just live receipt bytes; it is not a direct measure of saved e-graph payload.

The new objective and learned catalogs remain **opt-in**. New ordinary source
compilations retain their existing strategy. Full Cell registrations remain
968 / 319,326, logical fact creations remain 3,739 / 820,439, and the residual
candidate counter remains 12,275 / 4,490,637. This work improves the completion
mechanism and its measured coverage; it has not yet reduced those physical work
counts or delivered a large end-to-end gain.

## Cross-workload validation and limits

Transfer maps complete typed rule signatures, retains only unambiguous mappings,
reconstructs compositions on the target and checks every port plan again. It
transfers no e-class IDs, runtime facts or sampled values. The target retains its
own initial graph and existing catalog; compatible source seed compositions may
also be added as dependencies, so a transferred catalog need not have the source
catalog's size. Catalog budgets still apply.

| Training source → validation target | Final units | LHS attempts / hits | Function hits | Global probes, old / transferred |
|---|---:|---:|---:|---:|
| Raytrace → sum-bits | 310 | 22 / 18 | 7 | 19,352 / 19,330 |
| sum-bits → Raytrace | 301 | 731 / 727 | 158 | 4,423,086 / 4,422,359 |

The selected transferred catalogs' successful reads reach back one stage. The
selected self-trained Raytrace catalog additionally serves 53 two-stage reads.
A larger, unselected sum-bits catalog produced eight three-stage hits on Raytrace;
those are not attributed to the final selected artifact. No target measurements
were used to choose the source checkpoint.

Per-unit results are recorded in [ray-unit-work.json](iterative-lhs/ray-unit-work.json).
The two largest units serve 300 and 260 reads: generating the first `Get` of a
leading tuple and continuing its `Get` sequence. A retained commutativity pair
serves 139. These rule identities are an explanation of measured results, not
compiler admission tests. The remaining selected units serve 111 reads in total.

The new tests cover iterative bridges, function updates under `min`, deletion,
current-value guards, mixed query/effect receipts, source-rule and stage-distance
checks, rejected forged plans, transfer and the reference-preserving fallback.
All 27 compiler and 29 runtime tests pass with RHS-work/structural audit features;
the default build also checks successfully.

The workload audit compares full exported state, every scheduled Run, ordered
RHS inputs, effective writes, resume order and complete Spine-shape streams with
the frozen control. Only the artifact identity in the shape header is normalized.
Spine resumptions and longest chains remain 3,034 / 12 and 663,403 / 29. The
sum-bits forced hash/Sieve-collision audit also preserves all five full-stream
digests.
These are standalone Cell-versus-Cell checks, not a new proof of native egglog
equivalence. No reduction of logical facts or general mutable RHS fusion is
claimed by serving a row directly.

[Round histories](iterative-lhs/rounds-ray.json),
[selected catalogs](iterative-lhs/ray-catalog.json),
[bounded Cell e-graph](iterative-lhs/ray-cell-egraph.json),
[full audit digests](iterative-lhs/audits.json),
[timing summaries](iterative-lhs/results.json) and the two raw timing JSONL files
are retained in `docs/iterative-lhs`. Each workload has its own input and learned
catalog. The selected artifacts come from round one for sum-bits and round two
for Raytrace; the extra Raytrace exploration round is preserved as evidence.

## Cargo reproduction

Use fresh output directories and the frozen `multi-sum.json` / `multi-ray.json`
inputs from [multi-Run port reproduction](MULTI_RUN_PORTS.md#cargo-reproduction).

```sh
cargo run --release --locked -p eggfuse-cell-compiler \
  --features iterative-bake,two-run-blocks,view-ports -- iterate \
  --program out/multi-sum.json --output out/lhs-sum --objective lhs-lookups \
  --rounds 3 --max-depth 4 --max-nodes 8 --max-units 384 \
  --new-per-round 8 --candidate-slots 2048 --min-support 2
# Repeat with multi-ray.json and lhs-ray.

# Build runtime alone, separate from the compiler's serde feature union.
cargo build --release --locked -p eggfuse-cell-runtime \
  --features two-run-blocks,view-ports,storage-metrics \
  --bin eggfuse-cell-runtime --example bench_deferred_incidence --example transfer_lhs
target/release/examples/transfer_lhs \
  out/lhs-sum/program.json out/multi-ray.json out/sum-to-ray.json
target/release/examples/transfer_lhs \
  out/lhs-ray/program.json out/multi-sum.json out/ray-to-sum.json

# Identical learned catalogs: only LHS reads/retention differ.
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/lhs-own-pairs \
  out/lhs-sum/program.json out/lhs-ray/program.json 5 --iterative-lhs \
  out/multi-sum.json out/multi-ray.json
# Entire transferred catalog versus the original 64-unit program.
target/release/examples/bench_deferred_incidence \
  target/release/eggfuse-cell-runtime out/lhs-cross-pairs \
  out/ray-to-sum.json out/sum-to-ray.json 5 --catalog-pair \
  out/multi-sum.json out/multi-ray.json

# Build with slot-transfer-audit,spine-shapes added to the runtime features;
# use zero rounds and --audit-iterative-lhs / --audit-catalog-pair for full audits.
```
