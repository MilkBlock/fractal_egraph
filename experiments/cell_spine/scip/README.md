# SCIP and exact spine representation sharing

Measured on 2026-10-13 with the actual Cell compiler and native egglog closure
validation, using the current local kernel. This is not a pristine egglog speed
comparison. Each measurement is one run. Capture is reused and native compilation
is excluded. Source and binary hashes, versions and unrounded times are in
[`results.json`](results.json).

## Result

| Input / solver | History paths | Physical choices | Master vars / constraints | Peak inner vars / constraints | Native checks | Objective | Pre-export rows | Compiler time |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Boundary / SCIP 10.0.2 | 43 | 7 | 140 / 219 | 145 / 387 | 44 | 332 | 24 | 9.15 s |
| Boundary / Gurobi 13.0.3 | 43 | 7 | 140 / 219 | 145 / 387 | 44 | 332 | 24 | 6.55 s |
| eggcc If subset / SCIP | 10 | 5 | 46 / 115 | 389 / 702 | 11 | 2830 | 87 | 187.55 s |

Every native check passed full constructor/equality closure comparison. Checks
include all admitted history paths plus the current source input. Rows are the
source case's physical native rows before expansion, including auxiliary tables;
they are not expanded equivalent e-nodes or RSS. Objectives are structural costs,
not measured execution time. Model counts include generated value cuts.

The boundary catalog previously constructed an initial master with **2,324
variables and 3,903 constraints** before value cuts. Exactly identical typed
seed/root/interface representations now share one choice. All 43 history entries
remain obligations. The change does not discard middle-history Cells, rank paths
by frequency, or remove guards. The selected representation count is seven;
there are still three closure families, each choosing one representation.

An unnormalized SCIP attempt was manually stopped after more than 15 minutes and
has no successful result. It must not be reported as an optimum or a completed
timing. Reusing identical conditional pricing models alone did not make that
outer search practical. No such time cutoff is installed in the compiler.

| Work | Boundary / SCIP | Boundary / Gurobi | If / SCIP |
|---|---:|---:|---:|
| Layout profile construction | 5.59 s | 5.59 s | 184.31 s |
| Conditional marker solver | 2.660 s | 0.149 s | 2.158 s |
| Outer master solver | 0.025 s | 0.015 s | 0.001 s |

These times are components of compiler time, not additional phases. SCIP removes
the licensed size cap; it is slower than Gurobi on this boundary model. The If
case still pays for **107,668 layout evaluations**. Its first layout profile is
the remaining bottleneck. Neither the new backend nor exact model caching
eliminates the worst-case product of endpoint layout choices.

## Reproduce

Build the current native tools and install the default solver:

```sh
cargo build --locked --release --bin egg_layout \
  --example lhs_ast --example spine_validate \
  --example spine_stages --example native_math_baseline
python3 -m venv out/cell-spine-venv
out/cell-spine-venv/bin/pip install -r tools/cell_spine/requirements.txt
```

Capture once, explicitly including history windows, then compare both backends
on that same catalog. All output directories must be new.

```sh
EGG_LAYOUT_CELL_WINDOWS=1 EGG_LAYOUT_GRAPHS=0 RAYON_NUM_THREADS=1 \
  target/release/egg_layout analyze \
  --source experiments/cell_spine/fixtures/input_spine_boundary.egg \
  --output out/scip-compare-analysis

RAYON_NUM_THREADS=1 out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --solver scip --skip-build \
  --source experiments/cell_spine/fixtures/input_spine_boundary.egg \
  --analysis-dir out/scip-compare-analysis --output out/scip-compare

# Optional comparison; subject to the installed Gurobi license.
out/cell-spine-venv/bin/pip install -r tools/cell_spine/requirements-gurobi.txt
RAYON_NUM_THREADS=1 out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --solver gurobi --skip-build \
  --source experiments/cell_spine/fixtures/input_spine_boundary.egg \
  --analysis-dir out/scip-compare-analysis --output out/gurobi-compare
```

For the If case use `experiments/cell_spine/eggcc_if_tiny/source.egg` in both
capture and compile commands, with fresh directories. Ripen completion and
capture ordering can affect admitted catalogs; compare solvers on the same
catalog. Fresh automatic Bake enables windows with `EGG_LAYOUT_CELL_WINDOWS=1`;
without it, rooted capture remains the default for continuity with old workloads.

`--keep-duplicate-spines` is the explicit old-choice-space ablation. It can be
slow; no claim of polynomial optimization follows. The optional instance-storage
objective uses the new solver but deliberately does not reuse the structural
conditional-model cache.

## Regression coverage

- Incremental cuts and bound fixing after native solves; stale solutions cannot
  be read after edits or disposal. Infeasible is not accepted as optimal.
- SCIP solves 2,005 binary variables and 2,006 constraints without a license cap.
- The same API contract is checked with Gurobi when installed.
- Typed slot renaming may share a representation; different literals, guards,
  root order and certified root identity do not.
- A fresh full-window boundary Bake validates every history path and the source.
- CI v2 separately reports solver construction, solving and layout profiling;
  changing backend disables timing comparisons, not size/correctness checks.
