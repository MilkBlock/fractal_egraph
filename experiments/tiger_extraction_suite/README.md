# Shared cost bags: whole-suite pristine eggcc study

This is a CPU experiment on the **real three-pass eggcc pipeline**, followed by
paired extraction of its captured graphs. It is not an Analyze, Cell, GPU inference,
or synthetic graph benchmark. Only the sibling Tiger executable differs between
the two pipeline lanes. Both use the same pristine eggcc executable.

## Result

All **96** programs under upstream `benchmarks/passing` completed: 64 Bril, 30
PolyBench, raytrace, and Fenwick. All **288** pass graphs passed byte-identical
extraction output checks and the native cost/pick oracle (**439,249** class
entries). Every pipeline pair also had byte-identical intermediate graphs and
final output. There are no excluded failures or timeouts.

Ratios below are original / shared bag; greater than one favors shared bags.

| Measurement | Geometric mean over programs | Ratio of total times |
| --- | ---: | ---: |
| Global cost estimation | 3.818x | 7.032x |
| Complete standalone Tiger extraction | 1.052x | 1.104x |
| Complete eggcc pipeline, including graph capture | 0.995x | 0.978x |
| Peak standalone Tiger RSS | 1.010x | not aggregated |

Whole extraction totals **33.412 s -> 30.254 s**, saving **9.45%** of elapsed time.
92 program observations are faster and four slower. These are single paired
observations, not repeated estimates with confidence intervals.

Raytrace extraction improves **5.687 s -> 4.870 s (1.168x)**, but its observed full
pipeline regresses **52.607 s -> 62.149 s (0.846x)**. This remains in every total.
Across the suite, the driver's integrated extraction intervals total 37.955 s
and 38.058 s; they do **not** establish the standalone gain in situ. The driver
writes a temporary input and captures output through reader threads, whereas
standalone replay uses prepared input/direct output. This study does not quantify
the contribution of that envelope or claim end-to-end compiler acceleration.

## Method

The original global cost estimator copies hash maps of e-class contributions.
The candidate shares an immutable sorted vector and its cached sum. Repeated
keys retain the unsigned minimum; e-class identity, zero entries, overflow,
infinity, and the original scalar estimate are preserved. Merging creates a new
vector. Independently computed equal vectors are not content-interned.

Copying a bag changes from O(k) to O(1). Accumulation trades expected O(n) hash
updates for an O(m+n) vector merge, where m is the accumulator size and n the
child size. The gain comes from avoided copies, allocations, and scans; it is
not an asymptotic improvement for every operation. Statewalk, the final DAG
extractor, queues, tie rules, and the inference engine are unchanged.

Implementation: `tools/tiger_markers/cost_bag.h`, the isolated overlay in
`tools/tiger_markers/build.py`, and `tools/tiger_markers/extraction_markers.h`.
The measured mode is `bag`; optional CPU-recipe/Metal modes are disabled.

## Provenance and protocol

- eggcc: `ae80f82a403c1209fb084b115ee89470c7e66d0e`.
- Locked egglog-experimental: `08771f9f69de51cc8cdc58e3760b7f00a03e40d8`.
- Locked egglog kernel: `ebba7bb902bdc1b0f377b6bb22c06ac305912674`.
- Here, pristine means **unmodified upstream eggcc and its Cargo.lock engine
  stack**, not the latest upstream kernel or this repository's Cell/GPU fork.
- New clean checkout and fresh build. Remote Git fetching timed out, so verified
  Git objects came from a clean local mirror and registry archives were checked
  against the lockfile. No pre-existing eggcc binary was used.
- Debian 12 container; Rust 1.88.0; LLVM 18.1.8; GCC 12.2.0; i7-13700KF logical
  CPU 2 affinity; 24 GiB container memory and zero additional swap allowance.
- Both Tiger variants: `-std=c++17 -O3 -Wno-unused-result`, same compiler.
- Production `parallel` schedule and single-threaded egglog; upstream graph
  capture enabled in both lanes. 600 s process timeout, 8 GiB Tiger address-space
  limit. Failures are retained in inventory and have no fabricated speedup.
- One paired measurement per input/lane, alternating order by fixed inventory
  index. Native timer-reference and cost-oracle runs are separate from the
  primary pristine/bag elapsed-time comparison. GNU time measures process RSS.
- An earlier timer used POSIX `wait(timeout=...)`, which polls and can quantize
  short timings by up to 50 ms. **All external timings were invalidated and all
  96 programs remeasured** with blocking wait plus an independent watchdog.
  Only v2 timings appear here. Earlier output hashes are retained solely for an
  additional correctness repetition; all 96 repeated outputs/graphs agree.

## Data and report

- `results/workloads.csv`: every program and the three timing levels.
- `results/summary.json`: aggregate results and inventory provenance.
- `results/cases/*/result.json`: every pass's input hash, timing, RSS, cost/pick
  oracle counters, output hash, and pipeline checks.
- `results/manifest.json`, `machine.json`, `suite-command.json`: exact build,
  environment, affinity, source, binary and input fingerprints.
- `results/build-provenance/`: compiler/source receipts and arithmetic tests.
- `results/correctness-repeat.json`: preliminary output receipts, without times.
- `report/main.tex`: English technical report, with correctness argument,
  complexity analysis, figures, limitations, and all 96 rows in its appendix.

Raw graph snapshots, executables, and build trees are deliberately not in Git.
The retained final measurements correspond to remote
`/opt/eggcc-extraction-suite/results-v2`; the compact JSON records stand alone for
report generation. Existing fingerprint-identical runs can be resumed without
repeating the suite.

## Reproduce

Python 3.11+, a C++ compiler, and a clean locked release build of the pinned eggcc
are prerequisites. Linux does not need Swift or Metal. From the repository root:

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py \
  --repo "$PWD" extraction-suite \
  --checkout /path/to/clean/eggcc \
  --build-dir /path/to/new/tiger-build \
  --output /path/to/new/results
```

The source checkout is not modified. `--source benchmarks/passing/...` selects
an explicit pilot; omit it for the complete suite. `--report-only` regenerates
summary/CSV from an existing run. `--retry-failed` archives failed attempts before
retrying. The runner rejects incompatible cached fingerprints.

Generate the report from **committed data without running eggcc**:

```sh
python3 -m venv out/extraction-report-venv
out/extraction-report-venv/bin/pip install \
  -r experiments/tiger_extraction_suite/requirements.txt
out/extraction-report-venv/bin/python \
  experiments/tiger_extraction_suite/report_results.py \
  --input experiments/tiger_extraction_suite/results \
  --output experiments/tiger_extraction_suite/report
cd experiments/tiger_extraction_suite/report
pdflatex -interaction=nonstopmode -halt-on-error main.tex
pdflatex -interaction=nonstopmode -halt-on-error main.tex
```

Validation of benchmark plumbing:

```sh
python3 -m unittest discover \
  -s skills/cell-assisted-egglog-bench/scripts -p 'test_extraction*.py'
```

The C++ build also checks 4,000 randomized bag updates and 6,400 native candidate
cost cases. The report separates these preservation tests from measured speedups.
