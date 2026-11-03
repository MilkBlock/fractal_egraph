# Cross-stage execution of one Cell class learned from sum-bits

This experiment learns one guarded handoff from the **original frozen sum-bits Pass 3**. Training executes the entire native source schedule and samples connected pairs for preparation. The eight target programs are the existing marked Cell CPU programs, with the same deployed program and data on both sides of each comparison. The benchmark suite's small training fixture remains part of the common marker compilation and supplies no training records for the transferred unit.

## What the unit executes

The producer belongs to `always-run`. For the selected class, the producer read input index 2 and tuple length 5, then committed a `Get(tuple, 3)` row. Later an `is-resolved` consumer can enter from that row. It reads the current `ExprIsResolved(tuple)` port and, if present and visible, executes its original RHS to mark the Get result resolved. Failure of a class guard falls back to ordinary matching. The original Run controls publication.

The complete native capture had 4,444 records and 3,055 cross-ruleset write/read handoffs. For a bounded pilot, the learner intersects the native rules with the *training case's* marked typed rules, excludes initialization, and requires a uniquely lowerable edge with at least one residual instruction or reusable construction. It ranks by training frequency and prepares up to eight examples from each of the top eight eligible shapes. Four shapes and nine pairs qualified. The native preparation code and Cell e-graph registry produced 18 registered units in 14 classes. [Cell e-graph](cell-egraph.json). This is a sampled Cell e-graph of real rows, rather than an exhaustive ripening of every composition in the trace.

Among the prepared two-step classes, class 9 won the membership-count tie by registry order. Its sampled members are units 9 and 15. Unit 9 supplied the representative program. Their producer/consumer records were (715, 1370) and (474, 1383) at capture boundaries (79, 152) and (75, 154). The chosen representative saturated and passed its checks in isolation. Native endpoint certification was unavailable because primitive guards/actions require a separate semantics contract. This experiment executes the checked finite handoff and original effects. It grants no authority to substitute an entire saturated endpoint or to omit a source stage.

The frozen artifact carries the e-class provenance, relative parent binding, producer and consumer typed programs, declarations, training-observed literal guards and lowered instructions. Linking relocates exact rule identities and the target control fingerprint. It rejects incompatible rules or declarations. The ordinary runtime composition verifier checks the functional residual reads and effects. No other target is used to select the class. [Frozen unit](unit.json)

## Reproduce with Cargo

From the repository root, use fresh output paths.

```sh
cargo cell-compile --case sum-bits --output out/unit-training
EGG_LAYOUT_GRAPHS=0 cargo run --release --locked --manifest-path tools/cell-compiler/Cargo.toml --example baked_unit -- \
  learn out/unit-training/source.egg out/unit-training/program.json out/sum-bits-unit

cargo cell-compile --case fenwick_tree --output out/unit-target
cargo run --release --locked --manifest-path tools/cell-compiler/Cargo.toml --example baked_unit -- \
  link experiments/pass3_cell_transfer/unit.json out/unit-target/program.json out/linked.json
for enabled in 0 1; do
  CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1 \
    CELL_CPU_COMPOSITIONS="$enabled" cargo cell-run --program out/linked.json --output "out/unit-run-$enabled"
done
```

Use the same frozen file in the link command for every target. The runtime reports inference seconds separately from program loading, graph construction, checks and export. The physical cross-Run counter requires a selected consumer RHS application and a producer owner from an earlier epoch.

## Results

A single-threaded Apple M3 Max run used ten alternating measured pairs per case after one warm-up on each side. Raytrace used four measured pairs. All eight programs linked. The two lanes matched on the final full-state export and per-Run scheduling fields, logical matches and executed binding counts in every repetition. These checks compare the two standalone Cell CPU lanes.

| Target | Cross-Run applications | Candidate visits (both) | Unit off, ms | Unit on, ms |
| --- | ---: | ---: | ---: | ---: |
| fact | 0 | 3,317 | 16.05 | 15.69 |
| loopfact | 0 | 2,946 | 15.08 | 16.19 |
| sum-bits | 4 | 16,779 | 78.18 | 80.14 |
| dot-product | 0 | 14,700 | 65.58 | 68.89 |
| sum-divisors | 0 | 42,614 | 169.72 | 168.98 |
| fenwick_tree | 1 | 46,043 | 190.73 | 200.77 |
| jacobi-1d | 2 | 94,462 | 372.77 | 374.55 |
| raytrace | 24 | 9,908,673 | 34,912.86 | 34,805.65 |

The class fired 31 times across one complete run of all eight cases, including 27 applications on held-out workloads. Every accepted entry still needs a live consumer port lookup. The baseline already has a keyed lookup for that port. Candidate visits did not fall, and the timing pairs do not demonstrate a consistent inference speedup. Additional full-stage audits on sum-bits and fenwick_tree agreed on the initial graphs, all 3,550 Run graphs and binding sets, with four and one cross-Run applications respectively. Relearning from sum-bits reproduced the frozen file byte for byte. The experiment remains opt-in. [Measurements, including raw timings and input fingerprints](results.json)
