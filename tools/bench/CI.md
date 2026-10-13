# Cell size and phase CI

Workflow: `.github/workflows/cell-benchmark.yml` (**Cell size and phase benchmarks**).
It runs on relevant pushes to `master`/`main`, pull requests, and manual dispatch.
No published-site deployment or GitHub PR comments are involved.

The workflow uses Ubuntu 24.04, Rust 1.94.0, Python 3.12, and pinned PySCIPOpt
with its bundled SCIP solver. It needs no license secret or licensed model-size
allowance. There is no solver fallback, candidate budget, or approximate optimum
acceptance. Tests include a 2,005-variable / 2,006-constraint model and a fresh
full-window Bake, retaining validation of every admitted history path. Native tools build with `--locked` from checked-in sources. The job's
45-minute timeout is an operational CI stop, not a change to the algorithm.

## Measurements

Each case is freshly Baked from one seed. The suite covers left/right three-term
addition, external intermediate use, specialized repeated binding, a long spine
graft, and the eggcc If subset. Runtime uses 128 renamed copies of that seed.
Replication is allowed only for constructor-only source rules with no fixed
String rule literals, including generated export rules; native size checks then
verify the complete workload. This does not claim arbitrary unseen-input reuse.

Four modes use the **same current egglog kernel**, not pristine upstream:

- `native`: original rules and input.
- `raw_ingress`: generated library, ordinary forked input.
- `input_spine`: selected initial spine/marker representation, no expansion.
- `input_spine_export`: same representation followed by explicit expansion.

Every mode is timed **once**. Another process inspects sizes, so counting and
verification allocations do not inflate timed RSS. `wait4` measures the native
child's peak RSS (Linux KiB and macOS bytes are converted to bytes). No cumulative
previous-child RSS is used. No graph dump, SVG, or binary mutation stream is
uploaded. Logical matches are not described as committed mutations.

The Actions **Summary** contains:

1. All native table rows, raw/canonical e-nodes, live/retired markers, peak RSS,
   and expanded e-node/e-class counts. Full per-table counts remain in JSON.
2. Runtime read/install/input/run/export/drop phases, execution total and process
   wall time. Execution total is install + input + run + export. Lazy engine
   preparation may appear in Run, so this is a command-boundary decomposition,
   not a claim to isolate Free Join CPU time. A separate table uses egglog
   reports for search+RHS, merge and rebuild, for both Run and Export. Those
   subphases are already inside the command timers and must not be added again.
   No per-row instrumentation or new kernel timers are introduced.
3. Source preparation, tier-0 capture, catalog preparation, spine solve,
   library emission, input solve, verification preparation, native verification,
   and output phases. Spine solve includes pricing and preparation as well as
   native MIP solving. `--skip-build` excludes compilation; compiler process startup and final
   report serialization are visible only in process wall time.
4. Library/runtime/restore `.egg` bytes. File bytes, row counts and RSS are
   intentionally separate; a small descriptor is not claimed as heap compression.
5. Cost-mode metadata: native instance-pricing execution count,
   pricing seconds (already inside spine solve), and seed checkpoint Value slots.
   The seed score is not the replicated runtime RSS. Index shapes are reported
   in the compiler artifact but index byte capacities are not yet in the objective.

The full timing suite retains the default structural objective. A separate
small same-catalog `storage_pricing.py` ablation exercises explicit
`--storage-cost instances`, reports its additional Bake work and validates the
exported closure. Instance pricing is not yet cheap enough to silently replace
the full suite's previous optimization policy. Its reports are also uploaded
and appended to the job Summary; there is no hidden evaluation budget/fallback.

`ci_cases.json` records the workload and existing ripen limits: 50 ms, 200,000
facts, four steps per sweep. These are explicit benchmark settings, not new
candidate-selection limits. CPU, toolchain, source and binary hashes accompany
all results. The full native Bake validation remains in the compiler; replicated
workloads use count comparison with the same-input native run.

The summary also reports solver API construction and native solve time separately,
plus model counts, peak variables/constraints and layout evaluations. API timing
excludes Python expression construction. Preparation/profile/solver times are
already included in Bake phases. Equal structural model cache hits do not count
as new native solves. These measurements concern the actual Cell compiler and
native egglog validation, not a standalone synthetic optimizer alone.
The default layout pricer also reports source compilations/reuses and exact LHS
normalization reuse. Its exhaustive small-domain tests retain guards, binding
correlations and physical storage. Per-transition projection never removes a
global layout choice. The separate layout ablation can compare all three exact
pricing modes on a shared native analysis directory.

## Baseline and regression policy

Only the first successful **default-branch** run establishes the accepted cached
baseline. Passing runs do not gradually move it. Pull requests can read it but
cannot save it. A changed suite/fixture starts a separate cache namespace.
If the measurement schema changes, bump the report schema and cache namespace;
manual acceptance does not bypass incompatible/corrupt data. Cache expiry can produce a new first run; that run clearly reports that no
historical regression verdict was available.

Default alerts:

- Total table rows and generated file bytes: more than 5% growth.
- Individual runtime phases and totals: more than 30% **and** 2 ms growth.
- Individual Bake phases and totals: more than 30% **and** 100 ms growth.
- Runtime RSS: more than 15% **and** 8 MiB growth.
- Changed expanded node/class counts: fail rather than celebrate apparent shrinkage.

Reports now use `cell-ci-bench/v2` and the `cell-bench-v2` cache namespace; v1
baselines are not silently compared. Solver backend/version participates in the
environment fingerprint. Time/RSS gates apply only when CPU and toolchain/environment fingerprints match;
otherwise the report still shows measurements and enforces size gates. Even on
matching CPUs, shared-runner results are single-run alerts, not a statistical
significance claim. Missing cases/phases, invalid numbers, failed benchmarks,
and mismatched native/projected output sizes do not silently pass.

To intentionally accept a reviewed regression, use **Run workflow** on the
repository's default branch and select `accept_baseline`. Correctness and valid
measurement checks still must pass. The cache is updated only after a complete
successful run. Inspect `regression.json` for the exact metric changes; every run
uploads the summary, JSON, generated `.egg`, solver models and small logs for
30 days.

## Local reproduction

```sh
cargo build --locked --release --bin egg_layout \
  --example lhs_ast --example spine_validate \
  --example spine_stages --example native_math_baseline
python3 -m venv out/ci-venv
out/ci-venv/bin/pip install -r tools/cell_spine/requirements.txt
out/ci-venv/bin/python tools/bench/ci_cell_spine.py --output out/ci-cell
```

A quick smoke run can use `--cases left long_chain --copies 16`. It has a different
workload signature and must not be compared with the full CI suite.

```sh
out/ci-venv/bin/python tools/bench/check_ci_cell_spine.py \
  --baseline out/accepted/report.json --current out/ci-cell/report.json \
  --output out/ci-cell/regression.json --summary out/ci-cell/summary.md
```

The comparator never writes the baseline itself. Build outputs and benchmark
artifacts belong under ignored `out/` or `target/`, not in Git history.
