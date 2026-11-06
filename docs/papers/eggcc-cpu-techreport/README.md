# Eggfuse CPU architecture and eggcc benchmark artifact

Open [eggfuse.pdf](eggfuse.pdf) for the technical report. The source is
**eggfuse.typ**. The cover and Section 5 use the latest verified two-workload
performance checkpoint, **502e29cd**, from **evidence/performance-acceptance.json**.
The old performance tables and full-suite appendices have been replaced in the
PDF. Their original data remains in this archive for provenance and replay.
Section 8 contains the separately controlled memory study.

Rebuild the current report with

```sh
python3 reproduce/render-performance-data.py
python3 reproduce/render-memory-data.py
typst compile eggfuse.typ eggfuse.pdf
```

The performance medians are 16.379 ms for sum-bits and 6,140.271 ms for Raytrace.
Fixed-reference ratios are 2.056× / 2.200×; same-session original/candidate ratios
are 1.849× / 1.988×. Candidate builds use native CPU, fat LTO, one codegen unit
and profiles trained only on the opposite workload. These results cover two
frozen Pass 3 inputs. The current report does not substitute these times into
unmeasured workloads or the historical native egglog comparison.

## Data and historical archive

- [benchmark.csv](benchmark.csv) and [suite.jsonl](suite.jsonl) contain every survey result, flat and raw program digests, original benchmark source digests, offline preparation seconds, attached block counts, resumed steps, Cell bindings, final Cell state digests, native replay milliseconds, and archived production timing.
- **paired.json** contains the independently repeated Pass 3 experiments, including warm-up block zero. CLI measurements include each executable's normal reporting/export behavior. They have different output obligations. The report uses the internal inference timers for engine comparisons.
- **semantic-audit.json**, **grounded-check.json** and **witness-rows.json** document the sum-bits discrepancy. The frozen sum-bits Cell endpoint used for this check has the same SHA-256 as the sum-bits Pass 3 sweep row.
- **evidence/** contains binary / source fingerprints, manifests, the historical repeated anchor data, diagnostic logs, and the two compressed exports used in the grounded comparison. The archived native exporter uses the local native fork. The independent membership check was run with the tagged timing executable.
- **reproduce/captures.tar.gz** contains all captured raw and flat eggcc pass programs. The compact per-pass production report preserves aggregate fields. Full original report digests are in the manifest. The input programs are distributed under the included eggcc license. **donor.json.gz** contains the exact fixed 64-unit donor deployed in the survey.

## Replay the historical full-suite experiment

Clone [eggfuse](https://github.com/MilkBlock/eggfuse) including the `egglog-baseline` tag and checkout `70b291077252a8a6cfd9a00ebdd98f9599cd0c62`. Rust/Cargo, git, tar, gzip and Python 3.11+ are required. Run the following commands from the unpacked artifact in a clean shell. Output directories must be new.

```sh
sh reproduce/replay.sh /absolute/path/to/eggfuse out-one one
sh reproduce/replay.sh /absolute/path/to/eggfuse out-survey suite
sh reproduce/replay.sh /absolute/path/to/eggfuse out-anchors anchors
```

All compiler, engine and helper binaries are built with Cargo. The transfer helper uses the public Rust compiler and loader to reconstruct blocks. The Python driver selects inputs and times fresh processes. It runs serially, discards generated graphs after hashing and records failures per arm. Native consumes `pass-i-flat.egg` directly. Cell compiles that unmarked program with the shared marker training input and transfers the same donor to each target. The runtime envelopes, sample order and workload sizes are in the report. Compile and artifact-link costs are recorded separately from inference. To reproduce the host's scheduling priority, invoke the replay script under `nice -n 10`.

To reproduce the semantic witness, first run the single-case command. Then append the supplied assertion to the captured input and use the native executable built above.

```sh
cat out-one/capture/032-sum-bits/pass-3-flat.egg reproduce/sum-bits-witness.egg > out-one/witness.egg
out-one/cargo-target/release/examples/count_fj out-one/witness.egg
```

The timed tag executable returns `CheckError`. To inspect the grounded table difference with the archived native exporter, unpack the comparison inputs and run the read-only checker.

```sh
gzip -dc evidence/original-core.json.gz > out-one/original-core.json
gzip -dc evidence/native-export.json.gz > out-one/native-export.json
python3 reproduce/check-endpoint.py out-one/original-core.json out-one/native-export.json out-one/measured/state.json
```

The archived suite figures and derived tables can be regenerated with `python3 reproduce/figures.py` and `python3 reproduce/render-data.py`. Use the performance and memory renderers above for the current report. The PDF includes links to pinned source files and to the original egglog paper. This artifact was prepared for review and has not been submitted to upstream.

The packaged `one` command was executed from a fresh Cargo target. Its sum-bits result completed 1,428 Runs, reproduced the survey state hash, and reproduced the native witness failure. See **evidence/clean-reproduction.json**. Use `python3 reproduce/verify-data.py` to check the full data bundle.

## Memory update, 6 November 2026

Section 8 and the Chinese maintainer note report the separate two-workload
memory study based on `27f098a2`, implemented at
`4d9ea942b665a38df46ca673776e39314237841a` and documented at
`0f3a4fda4ddd4406e403f570a414395dc5908e73`. These commits belong to the local
`codex/cell-context-memory` research branch. The original full-suite source and
the grounded semantic witness have their own pinned provenance.

`evidence/memory-lifetimes.json` is an exact copy of that study's committed
results. It includes allocation snapshots, all three timing matrices, native
runner/source fingerprints, and the frozen Cell audit digests.
`memory-update.json` is a rendering derivative. Regenerate and validate it with

```sh
python3 reproduce/render-memory-data.py
typst compile eggfuse.typ eggfuse.pdf
```

To replay the control and combined memory configuration, check out the memory
implementation separately and supply existing frozen `program.json` inputs.
The script requires absolute paths and a new output directory. It records the
actual source commit and builds both executables with Cargo.

```sh
sh reproduce/memory-replay.sh /absolute/path/to/memory-worktree \
  /absolute/path/to/sum-bits/program.json \
  /absolute/path/to/raytrace/program.json \
  /absolute/path/to/new-memory-results
```

This is a one-run reproduction of each arm. The reported paired study rotated
fresh processes eight times per sum-bits arm and four per Raytrace arm.
Enable `allocation-census` only for the independent memory ledger, and
`slot-transfer-audit,spine-shapes` for ordered audit files. Diagnostic allocator
timings and RSS are excluded from the performance tables.
