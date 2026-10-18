# Marker alignment and physical layout selection

This is wired into the actual `matrix-numeric` native event/submission loop.
The existing `--gpu-check on` oracle checks complete bindings, primitive RHS
values and native merge before any GPU update is committed. Rule semantics,
stage ordering, alias guards and numerical precision are unchanged.

## Marker layout

`--marker-layout indexed` replaces the host's full fragment Cartesian product
with typed tuple indexes over admitted fragments. A marker is a versioned row
in assistant memory, with the existing Cell certificate/projection and bound
ports. It is **not** an extra egglog relation or a permanently true guard.

Indexes use all shared slots together, preserving correlations. An added or
changed fragment drives a delta join; when multiple fragments changed, the first
changed original part owns the result. Deleted/retired rows leave the indexes.
Union and function updates are canonicalized by native egglog before new frame
versions are admitted. Completed GPU channels stay immutable and are not replayed.
Parts with no shared slots retain their necessary Cartesian product.

This is still **CPU indexed matching**, not GPU Cell maintenance. Native point
reads/Cell revalidation remain host work. The initial implementation retains GPU
identity checks on aligned tuples, so the complete binding circuit stays
independently checkable. It does not pretend that moving an equality join into a
marker index eliminates all search cost. `--marker-layout cartesian` retains the
old path for ablation and remains the default.

The report counts marker additions/removals, index probes, visited indexed rows,
live index entries and emitted combinations. `cartesian_combinations_bound` is a
saturating diagnostic count, not a limit on matching. This index is incremental;
the upstream Cell admission pass still revalidates its active fragments.

## Matrix layout

`--matrix-layout` accepts:

- `legacy` (default): one square `XXᵀ` per feature shape.
- `paired`: requested pair rows only; exact squared differences over cached
  features, no Gram allocation, vector packing or host dimension readback.
- `solver`: choose disjoint paired and rectangular `LRᵀ` blocks with a MIP.

Features retain the previous exact FP32 bound. i64 guards/arithmetic and keyed
min/max use their exact integer kernels. Required conditions are never dropped;
Rust validates the selected blocks as an exact cover of every unknown condition.
Whole-binding OR branches, exposure depth and target gathers stay in the original
IR. The solver sees only widths and the required port-pair graph, not rule text,
numeric truth, or authority to certify a Cell. Duplicate native residual tests
share a condition ID when indexed markers are enabled.

The declared layout vocabulary is polynomial in required edges: the whole feature
shape, each connected component, batches of equal-dimension components, left/right
stars, and single comparisons. Each set has paired and rectangular alternatives.
The MIP chooses `x_b ∈ {0,1}`, with `sum(x_b for b containing c) = 1` for every
required comparison `c`, minimizing the sum of chosen layout costs. This is an
optimum **within this vocabulary and cost model**, not all possible abstractions.
The MIP itself can still take exponential time. No candidate truncation, automatic
time limit or silent backend fallback is used.

The initial cost model uses explicit **reference work units**, not measured GPU
nanoseconds. It prices dispatches, scalar work, GEMM multiply-accumulates, packing
and scratch traffic. Shared feature production is constant across alternatives
and uses the existing feature cache. Numerical DAG fusion, stage-selectivity
prediction, marker view selection and output layout optimization are not included.
Override all four weights with `--layout-costs file.json`, for example:

```json
{"dispatch":128,"scalar":1,"gemm_mac":0.125,"word":0.01}
```

The compiler keeps the current topology's plan and reuses it when only semantic
metadata grows. A changed comparison topology invokes the solver during plan
installation, not per match. Old whole-program plans are not accumulated.
Changing physical scratch layout preserves GPU evidence and completed channels.

SCIP is the default backend, reusing `tools/cell_spine/solver.py`. Gurobi is optional
and requires its license. A solver failure stops installation. Both require an
optimal result; floating-point solver tolerances still apply. Lowered/worker ABI
is **5**; rebuild `matrix.swift` and regenerate old lowered programs.

## Run

Add these to an existing `--mode matrix-numeric` command:

```sh
--marker-layout indexed --matrix-layout solver \
--layout-python /path/to/cell-spine-venv/bin/python \
--layout-solver scip --gpu-check on
```

`--layout-solver gurobi` uses the same model. Standalone `--mode matrix --backend
metal` also accepts the matrix-layout options and writes a sibling `.layout.json`.
The default Python is `python3`; use an environment containing the explicitly
selected solver. The existing installation instructions are in
[`tools/cell_spine/README.md`](../../tools/cell_spine/README.md).

One run per variant, using 12 actual analyzed Single/Get Cell instances, 36 offers
and 12 mutable Best entries:

```sh
python3 cell-assisted-egglog/validate_layout.py \
  --output out/marker-layout-check \
  --layout-python /path/to/cell-spine-venv/bin/python
```

By default the script checks SCIP and Gurobi; use `--solvers scip` if only SCIP is
installed. Native egglog and every GPU variant must have exactly equal final
graphs and primitive function values. All GPU runs use strict native checking.

## Observed work, not a speedup claim

| Variant | Candidate channels | GEMM result positions | Paired comparisons | Native accepted bindings / sets |
|---|---:|---:|---:|---:|
| Cartesian + square Gram | 10,368 | 5,184 | 0 | 24 / 12 |
| Indexed markers + square Gram | 72 | 5,184 | 0 | 24 / 12 |
| Indexed markers + paired | 72 | 0 | 48 | 24 / 12 |
| Indexed markers + SCIP | 72 | 0 | 48 | 24 / 12 |
| Indexed markers + Gurobi | 72 | 0 | 48 | 24 / 12 |

Marker indexing performed 252 probes and visited 432 indexed rows in this run.
The paired path processed 1,536 feature components and packed zero components,
versus 165,888 GEMM MACs and 2,304 packed components on the old path. These are
different operation counts, **not directly comparable execution times**.
Solver compilation happened twice in the incremental run and costs time too.
The example is a small correctness/work ablation, not full eggcc acceleration.

`tests/layout.rs` separately checks dense/sparse mixed blocks, swaps/moments,
whole-binding alternatives, target transport, updates and plan-cache reuse.
`test_layout_solver.py` compares both solver optima against exhaustive tiny-cover
enumeration. Native tests exercise late rows, union/rekey, deletion, no replay,
strict checks and intentionally corrupted GPU packets. See `results.json` for
the observed work counters; GPU buffer bytes are not RSS or egraph compression.
