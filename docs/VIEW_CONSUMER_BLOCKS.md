# Compiled Unit view producer and consumer blocks

The standalone Cell CPU compiler links a Unit view's external producer, consumer rules, RHS slot wiring and self-write projections into one checked plan in `view_consumer_blocks`. The loader verifies an emitted plan against the typed recipe, and derives it for older frozen artifacts. The experimental features are opt-in.

## Execution

A block requires a Unit function with old-value merge, one external copy/projection producer and multiple consumers in the same scheduled rulesets. The producer joins two constructor rows. Each consumer joins the view with the same constructor on a shared equality-sort column. The compiler lowers additional shared variables and supported i64 comparison predicates to port tests. At least one consumer must have a union-guarded column projection for the view. An extra unsupported rule reader, self-join, literal/repeated column within an atom, or an incoming scheduled Spine entry leaves the table on the ordinary route. All arm selection is structural, independent of rule and table names.

A consumer is activated only at its source Run and watermark. The matching code joins a fresh driver against any peer and a fresh peer against a strictly older driver. These two traversals are disjoint and cover both directions, including a late peer for a pre-existing view. The generic column arm verifies full equality after fingerprint lookup. With dedicated ports, the repaired publication passes each changed view witness to one exact join-key directory with an ordered epoch frontier. The directory retains an immutable canonical payload or a column projection over the producer payload. Hidden or removed inputs leave the appropriate domains. The peer constructor keeps its ordinary shared scheduling and indexes, also used by unrelated rules.

For disjoint `<`, `=`, `>` predicates over the same typed ports, one comparison selects the consumer. Other eligible predicate sets use individually checked guards, allowing overlapping outcomes. The executor buffers the receivers' ordered RHS inputs within the same Run and watermark. Each rule still commits at its original ordinal, with the original visible witnesses. A changed, deleted or replaced source reference must pass the complete live-key and pending-union checks before its projected write can reuse the source payload. The ordinary primary function table and logical witness IDs remain available to lookup, probes, repair, ordered writes and export.

## Frozen Pass 3

Both artifacts admitted the `Smaller` / `TCPair` producer (rule 287) and the three `CellSpineView9` consumers (509–511). The previous 64-unit Bake catalog, iteration schedule and Spine captures are common to all modes.

Initial component ablation against the same build with block execution off. The parent binary is included in the raw file.

| Mode | sum-bits median ms | Raytrace median s | Paired Raytrace delta ms, faster / 5 | Raytrace candidate visits | Generic view index updates |
|---|---:|---:|---:|---:|---:|
| Off | 32.867 | 12.856 | — | 7,263,030 | — |
| Projected storage only | 32.373 | 13.105 | +59.5, 2/5 | 7,263,030 | 287,634 |
| Compiled producer and consumers, generic columns | 31.118 | 13.688 | +114.0, 2/5 | 7,114,462 | 287,634 |
| Compiled block and dedicated ports | 30.942 | 13.123 | -287.4, 3/5 | 6,916,788 | 0 |

Final parent-versus-ports comparisons used two separate matrices with opposite starting orders.

| Matrix | sum-bits parent → ports (ms) | Paired delta (faster / 8) | Raytrace parent → ports (s) | Paired delta (faster / 4) |
|---|---:|---:|---:|---:|
| 1 | 33.022 → 32.941 | +0.310 ms (3/8) | 13.008 → 13.057 | -220.5 ms (3/4) |
| 2 | 31.915 → 31.898 | -0.439 ms (5/8) | 13.696 → 13.115 | -822.1 ms (4/4) |

Across the two final matrices, Raytrace was faster in 7/8 paired rounds with a median paired difference of −427.3 ms. The corresponding sum-bits result was 8/16 with a median paired difference of −0.003 ms.

In Raytrace the producer emitted 197,674 ordered inputs. The consumer join built its frontier 1,571 times and supplied another 3,142 rule calls from the same sealed snapshot. Exactly 148,010 pairs reached the three-way dispatch, producing 71,902, 75,550 and 558 RHS inputs. All 73,956 compact self-writes obtained the source directly from the block binding. On sum-bits the producer emitted 211 inputs and the three consumers received 49, 152 and 6. The storage-only and generic-column modes updated 287,634 view index rows on Raytrace and 306 on sum-bits. Dedicated ports eliminated those generic-column updates. The port directory still consumes repaired deltas and retains one logical handle per live view row.

In separate profiled processes, Raytrace producer search took 161.9 ms with storage alone and 125.9 ms with dedicated consumers. The three consumer searches together took 346.2 → 60.4 ms. Shared generation is attributed to the first requesting consumer. The dedicated view publication callback took 63.3 ms. These are medians of two instrumented samples, separate from inference timing. The complete final block consumed 287,718 view change headers on Raytrace, delivering 197,674 live payloads, and retained 73,956 port rows. Sum-bits consumed 315 headers, delivered 211 live payloads and retained 103 rows.

Raytrace ordinary Free Join entry visits fell from 6,790,332 to 5,748,328 and ordinary entry probes from 2,769,180 to 2,375,561. In exchange, the producer and consumers each process 197,674 driver-delta and 63,021 peer-delta inputs through their own bucket lookups. Per-directory comparisons and output preparation are included in inference. The existing index publication counter excludes these direct port deliveries. No whole-process memory reduction was measured.

The historical candidate counter includes residual/direct visits, shared node predicate checks, Spine pivot candidates and merge candidates. The block charges every opposite-bucket witness inspected to residual visits, including epoch rejections and fingerprint collisions. Frontier input enumeration, lookups returning no witness, comparison dispatch and publication are counted separately. A lower count need not imply a proportionate change in inference time.

## Verification and reproduction

Each release arm used fresh processes on an Apple M3 Max with rustc 1.94.0 and the same frozen programs. One warm-up per arm was excluded. Orders rotated and reversed. Inference includes the original schedule through repair and publication. Loading, graph initialization, export, audit and offline Bake are outside this timer. The main ablation used eight measured sum-bits rounds and five Raytrace rounds. That version delivered ports in a separate post-repair table pass. Final validation moved the same port update into the existing publication loop and compared the final executable directly with the parent using eight and four measured rounds per matrix, respectively. Raw independent medians, paired differences, CLI times and phase samples are in the results file.

All arms agree on the final graph, schedule, every ordered per-Run RHS input, effective function-write stream, applied Spine-resumption stream and complete winning prefix tree. Sum-bits executes 1,428 Runs with 3,034 resumptions at maximum depth 12. Raytrace executes 16,883 Runs with 663,403 resumptions at depth 29. A renamed fixture also checks every intermediate graph/binding stage under both full-width and zero-bit fingerprints, late arrivals, deletion/reinsertion and ordered accumulation. Compiler tests reject tampered dispatch and retain ordinary execution for unsupported additional readers.

From a fresh output directory, compile each input once and pass the same artifact to all arms. The normal runtime uses the original strategy by default. The change gives consistent reductions in candidate work and consumer matching time. With relative ordering and between-process variation visible in the two matrices, we keep it opt-in.

```sh
cargo cell-compile --case sum-bits --output out/sum
cargo cell-compile --case raytrace --output out/ray
cargo cell-bench --program out/sum/program.json --repeat 8
cargo bench --locked -p eggfuse-cell-compiler --features view-ports --bench cell -- \
  --program out/ray/program.json --repeat 4
```

For a fresh-process single execution, `cargo run --release --locked -p eggfuse-cell-runtime --features view-ports -- --program "$program" --output "$output"`. Use fresh output paths. Substitute `view-storage` or `view-consumers` for the other experimental arms. Add `slot-transfer-audit,spine-shapes` to capture ordered audit files. Use `CELL_CPU_PROFILE=1` in a separate run to collect producer and consumer search time and port maintenance time.

[Measurement samples, counters and audit digests](view-consumer-results.json)
