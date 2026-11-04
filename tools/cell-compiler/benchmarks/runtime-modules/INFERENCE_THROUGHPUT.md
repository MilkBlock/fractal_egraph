# Single-threaded inference on the frozen 64-block catalogs

The acceptance criterion is at least 2× the inference throughput of commit `ec56538` separately on sum-bits and Raytrace. Both releases execute the same 64-block input for each workload. The runtime starts a fresh graph, runs the entire original schedule, exports the final state and writes its report. The reported `inference_seconds` covers schedule execution. Compiling, training, loading, final export and independent diagnostics are outside this measurement.

| Workload | Measured pairs | Control inference median | Optimized inference median | Speedup | Pairs above 2× |
|---|---:|---:|---:|---:|---:|
| sum-bits | 9 | 79.34 ms | 37.07 ms | 2.140× | 8/9 |
| raytrace | 5 | 33.81 s | 15.50 s | 2.182× | 5/5 |

The benchmark used an Apple M3 Max with one excluded warm-up per mode and workload. Each measured sample was a new process. Pair order alternated, and the timed executables contained the iterative runtime with instrumentation features disabled. The speedup column divides the two medians. Per-pair ratios, phase medians and peak resident memory are in [the raw measurements](inference-throughput-results.json).

## What changed

- The compiler builds a register plan for each eligible pivot of a connected multi-table rule. A plan allows one indexed expansion, complete-key probes and bound primitives. It records which columns assign fresh slots and which must agree with existing values. The runtime uses scheduled seeds and the planned Free Join domains, enforces semi-naive ownership on the epoch, witness and pivot, and reuses a register array across candidates. Existing range selection and unsupported shapes keep their original path.
- The compiler also recognizes a single nonvirtual function write with a pure copy/primitive prefix and a scalar or unique merge. The executor reserves the original Cell ID and computes the tuple in a temporary frame. It stores a physical frame if the write changes the function's current owner. The same tuple and receipt pass through Spine's original capture and publication barrier. Both plan families are rederived when loading frozen programs and checked when present in a new artifact.
- Ordinary live facts and functions share immutable decoded fields. Union dependencies invalidate affected payloads. Repair, replacement and restoration reconstruct current values. Reference-view fact storage retains its projection semantics. Scalar function updates with unchanged eclass dependencies still publish a row change without removing and reinserting every reverse-use link.
- Column indexes update changed buckets and invalidate affected refinements. Sealed slots and Cell fact positions use ordered compact arrays. Lookup-only maps use a fast integer-oriented hash, while full values still resolve hash collisions. Stable sorting preserves RHS key order and deduplication priority. Both standalone runners and Cargo bench workers use mimalloc.

## Scope of the gain

Raytrace's register plans emit 2,650,667 bindings and its single-write plans avoid 1,017,798 transient frame registrations. Sum-bits emits 6,157 register answers and avoids 826 transient write frames. Actual constructor requests, logical facts, function changes and effective unions agree. The published candidate counter rises from 15,549 to 19,221 on sum-bits and from 7,778,113 to 8,647,393 on Raytrace. The register lane counts indexed witnesses and complete-key probe attempts, including misses. Graph key lookups rise from 19,188 to 20,088 and from 4,397,781 to 5,647,088 respectively. Median peak resident size for the complete process falls from 187.5 to 167.7 MiB on sum-bits and from 4,121.8 to 3,265.2 MiB on Raytrace. Lower cost per operation and reduced allocations account for the observed improvement.

Every timed sample matches its frozen schedule and final logical state. Separate untimed runs compare the byte sequence of ordered RHS-input hashes across all 1,428 and 16,883 Runs. The full winning cursor-prefix trees matched the prior frozen lineage on both inputs. The optimized runtime also resumed a control-build sum-bits checkpoint to the frozen endpoint. Resumptions and maximum depth remain 3,034 / 12 and 663,403 / 29.

## Reproduce

Use the two 64-block programs from [the iterative Bake workflow](ITERATIVE_BAKE.md) in both checkouts. Build each standalone binary with the respective locked manifest.

```sh
cargo build --release --locked --manifest-path engine/cell-runtime/Cargo.toml --features iterative-bake
export CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 CELL_CPU_SHARED_NODE_SCHEDULE=1
export CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1 CELL_CPU_FREE_JOIN=planned
export CELL_CPU_KNOWN_RESULTS=static CELL_CPU_RANGE_GUARDS=1 CELL_CPU_FD_KEYS=0
export CELL_CPU_SCALAR_MERGE=0 CELL_CPU_SLOT_TRANSFER=off
cargo run --release --locked --manifest-path engine/cell-runtime/Cargo.toml --features iterative-bake -- \
  --program "$program64" --output "$fresh_output"
```

Read `inference_seconds` from each report and rotate the two binaries across rounds. Use `iterative-bake,spine-shapes,slot-transfer-audit` in a separate build to reproduce the ordered RHS audit.
