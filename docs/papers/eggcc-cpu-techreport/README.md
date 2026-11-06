# Eggfuse CPU architecture and eggcc benchmark artifact

Open [eggfuse.pdf](eggfuse.pdf) for the technical report; its source is
[eggfuse.typ](eggfuse.typ). The main performance study compares **original native
egglog against the standalone Cell CPU engine**, across all **96 workloads and
288 frozen pass inputs**. Each input has three fresh-process measurements per
backend. Appendix A plots every named workload with both inference totals;
Appendix B lists all per-pass medians. No workload is excluded by its outcome.

The refreshed Cell source is `27f098a251b87449d25cc833aee7c32a7411d340`.
The native comparator is the immutable `egglog-baseline` tag,
`62ae6baa5539af4468a8f1a94d6e108de4dcd2df`. Both use one thread and ordinary Cargo
release builds, without PGO or machine-specific code-generation overrides.
Native executes the original **unmarked** input. Cell compiles that input using
one common marker-training source and one fixed sum-bits iterative donor.

Summed inference medians are **145.504 s native / 66.286 s Cell**, a **2.195×**
ratio. The equal-weight workload geometric mean is **2.197×**; all 96 three-pass
totals favor Cell. Excluding Raytrace gives **1.988×** by total inference time.
Summed CLI medians are 165.828 / 141.316 s (**1.173×**), before offline preparation
and with different output duties. Preparation adds 826.63 s for Cell artifacts.

The capture corpus comes from eggcc `ae80f82`, whose archived production run used
`egglog-experimental 08771f9`. Those production measurements are not the timing
baseline. Each replay begins at a captured pass boundary; the three-pass sum is
not a new end-to-end compiler run driven by either backend's extraction.

## Regenerate the report and verify data

```sh
python3 reproduce/render-current-data.py
python3 reproduce/render-memory-data.py
python3 reproduce/verify-data.py
typst compile eggfuse.typ eggfuse.pdf
shasum -a 256 -c SHA256SUMS
```

- [suite-current.jsonl](suite-current.jsonl): all three pairs for all 288 inputs,
  medians, source/program hashes, preparation costs, deployed units, Cell Run
  counts, bindings, resumed steps and endpoint hashes.
- [benchmark-current.csv](benchmark-current.csv): one row per workload and pass,
  including inference, CLI, RSS and preparation measurements.
- [current-report-data.json](current-report-data.json): derived tables, aggregate
  and equal-weight ratios, full workload names, tails and anchor measurements.
- [evidence/suite-current-provenance.json](evidence/suite-current-provenance.json):
  source, binary and training identities, build settings and measurement protocol.
- `figures/workloads-current-1.svg` through `-4.svg`: four named workload plots
  using one common log-ratio scale. `figures/suite-current.svg` is the overview.

All 288 inputs completed. Final Cell endpoint hashes equal the archived Cell
endpoints, and repeated processes agree on the graph, schedule hash, binding
count and resumed-step count. These checks establish consistency within Cell.
**They do not resolve the native/Cell membership difference in Section 7.**
The concrete sum-bits witness remains an acceptance gate for a backend change.

## Reproduce the current native comparison

Requirements: Rust/Cargo, git, tar, gzip and Python 3.11+. Check out the measured
Cell revision separately, including the immutable native baseline tag. Run from
this artifact directory with a new output directory:

```sh
sh reproduce/refresh-suite.sh /absolute/path/to/eggfuse-at-27f098a2 \
  /absolute/path/to/new-native-cell-results
```

The script builds the compiler, engine, native adapter and catalog linker with
Cargo. Its serial Python driver compiles/links each target once, alternates
three fresh native/Cell process pairs, and discards graph exports after hashing.
Pass order is P3, P1, P2. There is no excluded warm-up per input. Preparation is
outside inference; sums and ratios are derived from per-pass medians. The
unisolated workstation and three repetitions do not support strong claims about
small timing differences.

Native inference starts after initialization at the first RunSchedule. Cell
inference includes scheduled search, RHS, repair and publication. CLI includes
each tool's normal loading and output obligations; Cell exports the entire graph
and report, while the native helper does not export. Native peak RSS covers the
whole process, while Cell reports the peak through inference before export.

The bundle includes all captured raw/flat programs in `reproduce/captures.tar.gz`
and the fixed donor in `reproduce/donor.json.gz`. `evidence/manifest.json` preserves
source identities and input digests. The accompanying eggcc license applies to
the captured input programs.

## Semantic witness

The archived Cell and native exports have equal projected table counts, but a
grounded TermSingle key is visible only in Cell. Appending the assertion to the
original source fails in the native timing executable. After reproduction:

```sh
cat /absolute/path/to/new-native-cell-results/capture/032-sum-bits/pass-3-flat.egg \
  reproduce/sum-bits-witness.egg > /absolute/path/to/new-native-cell-results/witness.egg
/absolute/path/to/new-native-cell-results/native \
  /absolute/path/to/new-native-cell-results/witness.egg
```

The expected result is `CheckError`. `semantic-audit.json`, `grounded-check.json`,
`witness-rows.json`, the compressed exports in `evidence/`, and
`reproduce/check-endpoint.py` retain the grounded comparison. The report proposes
an alternative matcher inside native semantic ownership as the first upstream
experiment; it does not claim a verified drop-in backend replacement.

`evidence/suite-current-reproduction.json` records a successful one-pair smoke
test of the packaged driver and a fresh native witness failure. This extra pair
is outside the three-pair full-suite measurements. The exact measured driver is
archived as `evidence/suite-current-driver.py`; the portable entry point is
`reproduce/refresh-suite.py`.

## Separate memory study

Section 9 uses its own ordinary-release paired control at `27f098a2`, with
lifetime changes implemented at `4d9ea942b665a38df46ca673776e39314237841a` and
documented at `0f3a4fda4ddd4406e403f570a414395dc5908e73`. These commits belong to
`codex/cell-context-memory`. Do not combine this study's lower RSS with the
96-workload suite's faster times as if they described one measured executable.

`evidence/memory-lifetimes.json` preserves all three matrices, allocation
snapshots, native source/binary fingerprints and Cell audit digests.
`memory-update.json` is a derived rendering. To replay the two memory arms,
supply frozen Cell program files and a fresh output directory:

```sh
sh reproduce/memory-replay.sh /absolute/path/to/memory-worktree \
  /absolute/path/to/sum-bits/program.json \
  /absolute/path/to/raytrace/program.json \
  /absolute/path/to/new-memory-results
```

This command executes one process per arm. The reported matrix rotated eight
processes per sum-bits arm and four per Raytrace arm. Allocator instrumentation
and ordered audits use separate diagnostic builds; their timing and RSS are
excluded from the performance table.

## Preserved historical evidence

`benchmark.csv`, `suite.jsonl`, `report-data.json` and `paired.json` retain the
original `70b2910` survey and repeated anchors. `reproduce/replay.sh`,
`reproduce/sweep.py`, `reproduce/render-data.py` and `reproduce/figures.py` replay
or render that original study. `evidence/clean-reproduction.json` records its
fresh-Cargo single-case reproduction. The production profiles remain archived
with the captures; they are not mixed into the refreshed replay medians.

`evidence/performance-acceptance.json`, `performance-update.json` and
`reproduce/render-performance-data.py` preserve a separate two-workload Cell
checkpoint at `502e29cd`, using cross-workload PGO, native CPU and fat LTO. It is
not the native baseline and does not supply the current PDF's main comparisons.

The artifact is prepared for review. No upstream submission or publication is
performed by these scripts.
