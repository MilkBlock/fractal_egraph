# Eggfuse CPU architecture and eggcc benchmark artifact

Open [eggfuse.pdf](eggfuse.pdf) for the technical report. The source is **eggfuse.typ** with vector figures in **figures/**. Appendix A lists matched, fresh egglog and Cell inference times for every passing workload at the pinned eggcc revision. Appendix B lists the historical native execution of the actual eggcc pipeline.

## Data

- [benchmark.csv](benchmark.csv) and [suite.jsonl](suite.jsonl) contain every survey result, flat and raw program digests, original benchmark source digests, offline preparation seconds, attached block counts, resumed steps, Cell bindings, final Cell state digests, native replay milliseconds, and archived production timing.
- **paired.json** contains the independently repeated Pass 3 experiments, including warm-up block zero. CLI measurements include each executable's normal reporting/export behavior. They have different output obligations. The report uses the internal inference timers for engine comparisons.
- **semantic-audit.json**, **grounded-check.json** and **witness-rows.json** document the sum-bits discrepancy. The frozen sum-bits Cell endpoint used for this check has the same SHA-256 as the sum-bits Pass 3 sweep row.
- **evidence/** contains binary / source fingerprints, manifests, the historical repeated anchor data, diagnostic logs, and the two compressed exports used in the grounded comparison. The archived native exporter uses the local native fork. The independent membership check was run with the tagged timing executable.
- **reproduce/captures.tar.gz** contains all captured raw and flat eggcc pass programs. The compact per-pass production report preserves aggregate fields. Full original report digests are in the manifest. The input programs are distributed under the included eggcc license. **donor.json.gz** contains the exact fixed 64-unit donor deployed in the survey.

## Rebuild and run

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

Run `python3 reproduce/figures.py`, `python3 reproduce/render-data.py` and `typst compile eggfuse.typ` to rebuild the report from the supplied data. The PDF includes links to pinned source files and to the original egglog paper. This artifact was prepared for review and has not been submitted to upstream.

The packaged `one` command was executed from a fresh Cargo target. Its sum-bits result completed 1,428 Runs, reproduced the survey state hash, and reproduced the native witness failure. See **evidence/clean-reproduction.json**. Use `python3 reproduce/verify-data.py` to check the full data bundle.
