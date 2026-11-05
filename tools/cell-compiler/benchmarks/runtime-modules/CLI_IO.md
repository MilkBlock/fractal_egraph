# Loading and exporting a full Pass 2 graph

The control is the standalone Cell CPU executable at a85b8ea2. Both lanes use the same transferred catalog, computed keys and catalog-scoped publication on the frozen 2mm and 3mm Pass 2 artifacts. Each run reads the JSON artifact, loads and validates it, initializes a new graph and executor, executes the full schedule, writes state.json and report.json, then exits. Compilation, offline Bake and output hashing are outside the measured process. Files and page cache are warm, each output directory is new. Results are local to this workstation.

## Ordinary CLI wall time

Four measured fresh-process pairs after one excluded warm-up per arm; timers disabled on stderr. Milliseconds. Paired delta is optimized minus control within each matched round.

| Case | Before | After | Median paired delta | Faster |
|---|---:|---:|---:|---:|
| 2mm | 3,829.00 | 3,094.55 | -734.45 | 4/4 |
| 3mm | 4,533.38 | 4,082.63 | -478.41 | 4/4 |

## Phase attribution

A separate six-pair experiment enables the same diagnostic clocks in both builds. Load includes reading, JSON decoding and validating the typed source and executable plans. Setup includes graph construction and scheduler initialization. Export includes constructing and flushing the complete logical graph file. Drop covers releasing the executor and the report. Outside is process wall minus inference per sample and includes OS process overhead. Medians of individual phases need not add to the wall median. [All samples and output digests](cli-io-results.json)

| Case | Phase | Before, ms | After, ms | Median paired delta, ms |
|---|---|---:|---:|---:|
| 2mm | load | 99.36 | 74.84 | -24.38 |
| 2mm | setup | 1.49 | 1.36 | -0.16 |
| 2mm | export | 620.79 | 97.80 | -519.06 |
| 2mm | report_build | 19.94 | 19.62 | -0.34 |
| 2mm | report_finish | 4.54 | 4.92 | +0.19 |
| 2mm | drop | 101.69 | 106.95 | +6.23 |
| 2mm | inference | 2,733.40 | 2,747.91 | +14.90 |
| 2mm | outside | 869.37 | 322.79 | -530.69 |
| 2mm | wall | 3,601.89 | 3,073.35 | -528.04 |
| 3mm | load | 109.16 | 81.05 | -28.27 |
| 3mm | setup | 1.50 | 1.35 | -0.14 |
| 3mm | export | 741.36 | 117.52 | -632.49 |
| 3mm | report_build | 25.07 | 24.61 | -0.34 |
| 3mm | report_finish | 4.63 | 5.01 | +0.38 |
| 3mm | drop | 133.75 | 131.70 | -0.48 |
| 3mm | inference | 3,636.25 | 3,628.34 | -7.90 |
| 3mm | outside | 1,037.00 | 383.29 | -662.35 |
| 3mm | wall | 4,693.56 | 4,011.38 | -693.98 |

## Implementation and checks

The loader compares metadata, slot-to-source operands, effects, ruleset membership and continuation-slot certificates using borrowed JSON values. It avoids building a second tree of decoded rules and copying it into every ruleset. Immutable shared-entry wiring is shared with scheduler instances. Bootstrap uses a table-name index and moves the original initial records through import. Checkpoint restore initializes an empty graph without cloning the entire program and decodes fields from borrowed JSON.

A table exporter pre-encodes invariant field names, the table name, visibility layout and column sort metadata. Scalar and identity payloads stream into a 128 KiB write buffer. Text escaping uses serde_json; sets and structural keys use the existing recursive value representation. Export walks existing ordered witness indexes. It retains hidden rows, canonicalized identities, numeric float payloads, Unicode escaping and anchor order. The artifact and wire schemas stay compatible, including builds where Cargo feature unification changes JSON object key order.

Each timed trial checks the complete state file hash. Separate audit builds match the prior ordered RHS digest, final graph and schedule for all 3,658 and 3,905 Runs. Tests cover malformed source metadata, operands and effects, wire values including structural keys, and object ordering in the compiler build. A sum-bits checkpoint produced by the control at Run 800 resumes to the control's full state hash. Timing samples compare the standalone runtime only; this experiment changes the cost of running the same Cell program.

For the original program inputs and transfer commands see the [CPU report](https://github.com/MilkBlock/eggfuse/pull/33) and [computed-key experiment](COMPUTED_KEYS.md). Using the latter's environment (including computed keys and catalog publication), run the built release executable directly to exclude Cargo launch time.

```sh
cargo build --locked --release --manifest-path engine/cell-runtime/Cargo.toml --features iterative-bake
CELL_CPU_CLI_TIMING=1 engine/cell-runtime/target/release/eggfuse-cell-runtime \
  --program "$ready" --output fresh-output
```

With a custom CARGO_TARGET_DIR use its release binary. The extra stderr JSON reports seconds for read, parse, validate, graph, executor, setup, inference, export, report_build, report_finish and drop. A resume reports combined setup time. Use an external process timer to measure launch through exit.
