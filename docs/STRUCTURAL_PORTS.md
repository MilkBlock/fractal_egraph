# Cross-Run completion with deeper producer structure

These experiments use the standalone single-thread Cell CPU engine with the two frozen Pass 3 programs, their 64 iterative Bake units and the view-ports configuration. Structural depth counts witnessed edges between rows. Run distance and Spine continuation length are separate quantities.

## Execution contract

The compiler builds a finite graph for each producing rule. Vertices denote LHS table atoms and RHS table operations. Edges share a typed equality slot, including identities proved from Copy, explicit union and constructor congruence inside the producer. Source and destination table columns identify an edge. Producer fragments exceeding the 64-node or 192-operation limit are excluded. This is a bounded search over the lowered rule and actual invocations. Other equivalent representations in the offline Bake e-graph could yield additional paths.

The broad trial captures references from actual matched rows and RHS receipts. A cell can append a previously published suffix from another producer. Each anchor carries up to eight references in total. Publication follows repair and same-key forwarding. Consumption rechecks the complete key, current visibility and any required output equality or primitive guard. The normal scheduled entry remains available on a miss. A closed RHS preflights every constructor receipt, preserves explicit unions and keeps the original write order. Skipping a constructor request and removing a logically visible fact have different semantics.

The survey observes the unchanged execution and records ports supplied on original bindings. It groups each binding by whether it really resumed a Spine cursor. The execution trial also measures successful substitutions for table probes, retained ordinary-entry bindings, accepted RHS receipts and whole RHS completions. Historical root contexts, raw emitted matches and logical writes are counted separately. A survey may recover a missing anchor for diagnosis. Both frozen programs needed zero such reads. Compare separate depth-limited runs for marginal coverage.

## What depth bought

| Case | Depth | Replaced Spine table probes | RHS completions outside / inside Spine | Constructor requests | Peak retained port references | Median inference |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits | 0 | 0 | 0 / 0 | 11,502 | 0 | 30.86 ms |
| sum-bits | 1 | 50 | 284 / 97 | 10,898 | 9,215 | 38.62 ms |
| sum-bits | 2 | 192 | 439 / 143 | 10,465 | 22,066 | 44.84 ms |
| sum-bits | 3 | 221 | 476 / 150 | 10,422 | 26,290 | 46.15 ms |
| Raytrace | 0 | 0 | 0 / 0 | 2,065,807 | 0 | 12.99 s |
| Raytrace | 1 | 2,218 | 43,996 / 7,641 | 1,949,540 | 2,439,818 | 16.86 s |
| Raytrace | 2 | 234,405 | 64,211 / 12,868 | 1,912,361 | 5,720,173 | 19.78 s |
| Raytrace | 3 | 307,305 | 69,732 / 13,764 | 1,906,158 | 6,437,149 | 21.19 s |

The independent depth-3 survey found 16,669 non-resumed and 316,085 resumed LHS ports on Raytrace. It proved a complete multi-atom LHS for 0 and 312,793 bindings in these two groups. On sum-bits it found 31 and 314 ports and 0 and 267 complete LHS bindings respectively. The additional ordinary scheduled register entry hook retained zero complete bindings on either input. These figures describe this entry order and bounded encoding.

On Raytrace the operation on the `Expr-size(x)` anchor provides a receipt for `Get(t,i)=x`. The receipt can in turn carry `Single(x)` from an earlier cell. The profiled two-edge path for the `Single` atom of rule 155 occurred 220,975 times on resumed applications. This count describes available ports on already scheduled bindings. Other histories can install the same port. Length three also supplied a `Get → Get` intermediate on 56,952 observations for that atom.

The depth-3 trial created 62,832 fewer physical cells on Raytrace. Logical fact creation remained 820,439 and effective unions remained 226,523. In an instrumented run, collecting references took 2.65 s and publishing them took 2.74 s. The uninstrumented median includes this work, the consumer checks and fallback.

## Bake just the selected ports

Each depth-3 survey trains a portable catalog from its resumed LHS port observations. Bake selects the eight most frequent table-and-column paths at depth two and another eight at depth three. For the other workload, the compiler resolves these names and columns against its own typed plan. For every selected path and suffix, it emits direct producer copies and copies from a previously published suffix. Registering a source requires a live receipt for each intermediate witness. The consumer attempts the selected path when its normal Spine entry reaches the target table. This trial targets LHS reads and executes the original RHS. Its table-probe counts can overlap differently from the broad trial because selection changes how the eight references are allocated.

| Train → execute | Paths accepted | Replaced table probes | Peak references | Paired control → selected median inference | Control → selected median CLI |
|---|---:|---:|---:|---:|---:|
| Raytrace → sum-bits | 16 / 16 | 155 | 560 | 30.63 → 32.91 ms | 134.57 → 140.55 ms |
| sum-bits → Raytrace | 16 / 16 | 303,355 | 48,166 | 12.922 → 13.438 s | 15.429 → 15.881 s |

The selected engine on Raytrace achieved 98.7% of the broad D3 probe replacement count with 0.75% of its peak retained references. It generated 1,516,143 candidate packets, including empty and repeated publications, and published 609,955 nonempty winning packets after repair. In a separate profile, capture and publication took 249 ms and 289 ms. The median of the five paired inference differences was +502 ms (four slower). On sum-bits all seven selected runs were slower, with a median paired difference of +2.05 ms. Absolute timings across the broad and selected matrices have separate controls.

The follow-up measured identical proposals, attached the selected slots to the producing receipt, and deferred the copies until after the last writer was known. [Receipt slot experiment and paired results](RECEIPT_SLOTS.md).

Both workloads and all tested depths matched the frozen SHA-256 digests for ordered per-Run RHS inputs, effective writes, resumptions, Spine shapes, schedule and final graph. The run counts were 1,428 and 16,883. Resumed steps were 3,034 and 663,403, with longest chains of 12 and 29. The JSON file alongside this report contains raw timings, phase profiles, learned catalogs, path counts and reference digests.

## Reproduce with Cargo

Run these commands from the branch root with fresh output directories.

```sh
cargo cell-compile --case sum-bits --output out/sum-bits
cargo cell-compile --case raytrace --output out/raytrace

cargo run --release --locked -p eggfuse-cell-runtime --features view-ports,structural-depth -- \
  --program out/sum-bits/program.json --output out/train-sum --structural-depth 3 --structural-survey
cargo run --release --locked -p eggfuse-cell-runtime --features view-ports,structural-depth -- \
  --program out/raytrace/program.json --output out/test-ray --structural-depth 3 \
  --structural-catalog out/train-sum/structural-catalog.json

cargo bench --locked -p eggfuse-cell-compiler --bench cell --features view-ports,structural-depth -- \
  --program out/raytrace/program.json --structural-depth 3 --repeat 5 \
  --structural-catalog out/train-sum/structural-catalog.json
```

Exchange the case names to test the reverse transfer. Omit the catalog and set depth to 0, 1, 2 or 3 for the broad ablation. The timing matrix used a fresh CLI process per measurement. For ordered audit files, add the runtime features `slot-transfer-audit,spine-shapes`. For capture/publication timers, use a separate run with `CELL_CPU_PROFILE=1`. The default runtime keeps this experiment off.
