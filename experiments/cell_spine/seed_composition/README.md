# Small Cells can compose across a late constructor boundary

The production compiler now emits additional transitions from certified seed
cuts. This uses the existing native-parser AST, closure certificates, spine
lowerer and joint optimizer. It adds no egglog kernel primitive or Add-specific
compiler. The source of each cut is recorded in `transitions.json`.

## What is proved

If a known larger seed contains `Join(A(x), A(y))`, and the catalog has a
certified Cell for `A(p)`, the compiler can use two instances of that template
with distinct bindings `p=x` and `p=y`. It retains the connecting `Join` fact
and any other external facts/equalities required by the larger seed.

The source seed is a typed syntactic substructure of the initial target seed;
every explicit source guard is also an initial target guard. The target's
positive constructor/equality closure has already been certified under the
same rules. No equation from future saturation is assumed as an input guard.
Existing observed interfaces remain available. New cuts are additional runtime
contracts, so their costs are included rather than silently replacing a required
interface. They do not create new trace events or change the Rust history DAG.

This covers known single-root child Cells at actual constructor boundaries.
It is not arbitrary graph partitioning, all parent subsets, or new target-cell
discovery. Multi-root source Cells keep their existing observed transitions.
Equal child positions in the seed are one instance; a general emitted rule can
still legally bind both runtime ports to one instance, tested below.

## Automatic Bake and native regression

[`add2_pair.egg`](../fixtures/add2_pair.egg) uses commutativity and the balanced
instance of associativity. Its native closure has 26 e-nodes / 11 e-classes.
This is a deliberately small subset, **not** the full AC closure. The compiler
does not recognize this ruleset specially.

```sh
EGG_LAYOUT_CELL_WINDOWS=1 out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/fixtures/add2_pair.egg \
  --cell-composition seed-cuts --output out/my-add2-merge --skip-build
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_composition
```

The automatic example admits 26 historical paths and adds 15 certified cut
interfaces. All 27 native seed/source checks pass. Additional regressions verify:

- Two already compact Add2 spines plus a new raw connector reach the complete
  larger closure; actual `cut_*` rules match, and the two old markers retire.
- Two disconnected compact Add2 instances do not create Add4.
- After one small marker retires, its shared subtree remains usable through
  the larger owner when a second connector arrives.
- Both runtime source ports may alias the same small instance.
- Non-Add constructors, incompatible sorts, absent equality guards and missing
  connectors are distinguished by the matcher.

## One-run measurements (2026-10-13)

The existing native benchmark harness measures the current local kernel, not
pristine upstream. One run per mode; counts and per-rule diagnostics are in a
separate process. Timings do not include native build. The source/seed checks use
full closure comparison; replicated runtime cases compare actual exported
node/class counts. No inference speedup is claimed.

Ordinary complete input, 128 renamed copies:

| Compiler mode | Bake | Runtime rules | Library bytes | Physical rows | Input-spine Run |
|---|---:|---:|---:|---:|---:|
| Rooted history, observed parents | 0.924 s | 10 | 7,630 | 1,920 | 1.141 ms |
| Windows, observed parents | 1.097 s | 10 | 7,630 | 1,920 | 1.079 ms |
| Windows + seed cuts | 3.453 s | 62 | 48,589 | 2,048 | 7.065 ms |

Direct initialization already builds the large Cell here, so this workload does
not need the new route. The extra inherited consumers add real overhead. The
raw-ingress run does match the new multi-marker routes; the direct initializer
need not. These are different paths and are reported separately. Source-only facts whose values
are already supplied by a certified Cell are no longer rematerialized just to
link aliases; target spine construction and necessary unions remain explicit.

More importantly, introduce the bridge **after** two compact Add2 inputs exist:

| Runtime, 128 pairs | Complete result? | Expanded nodes / classes | Physical rows | Bridge + Run |
|---|---|---:|---:|---:|
| Native, small graphs pre-saturated | Yes | 3,328 / 1,408 | 4,224 | 5.776 ms |
| Earlier observed-only compact library | **No** | 1,280 / 896 | 2,304 | 4.331 ms |
| New seed-cut compact library | Yes | 3,328 / 1,408 | 2,560 | 10.762 ms |

The earlier library also fails a single-instance full native closure check on
this input; its lower time and smaller graph must not be described as savings.
The new route closes this semantic gap. It retains fewer physical rows than the
native run but is slower. The 62 rules include inherited routes for small Cells
whose markers have retired; simply deleting those routes would break shared
subtree continuation. Their expansion is a remaining performance problem.

Reproduce both tables:

```sh
out/cell-spine-venv/bin/python experiments/cell_windows/spine_bench.py \
  --cases add2_pair --copies 128 --output out/add2-merge-bench
RAYON_NUM_THREADS=1 out/cell-spine-venv/bin/python \
  experiments/cell_spine/seed_composition/late_bench.py \
  --bake-root out/add2-merge-bench --copies 128 --output out/add2-late-bench
```

## Standard AC4: semantic verification, not an optimum

[`add2_pair_ac.egg`](../fixtures/add2_pair_ac.egg) has the ordinary commutativity
and associativity rules. Its full closure is **54 e-nodes / 15 e-classes**.
A full automatic optimization attempt was stopped while still constructing its
first layout profile; it produced no optimum or completed Bake report. No such
cutoff or candidate truncation is installed in the compiler.

An explicit verification program selects legal fixed canonical layouts while
keeping all admitted history paths and all cut interfaces. The generated library
has 2,266 rules; it is not an optimized artifact. Starting from two compact Add2
instances plus the connector, native full-closure validation succeeds with all
54 e-nodes restored, without executing the original rules on the compact path.
This demonstrates the standard AC semantics, not satisfactory library size or
fast automatic optimization.

```sh
EGG_LAYOUT_CELL_WINDOWS=1 EGG_LAYOUT_GRAPHS=0 RAYON_NUM_THREADS=1 \
  target/release/egg_layout analyze --source experiments/cell_spine/fixtures/add2_pair_ac.egg \
  --output out/ac4-analysis
out/cell-spine-venv/bin/python experiments/cell_spine/seed_composition/verify_fixed.py \
  --analysis-dir out/ac4-analysis --output out/ac4-fixed-check
```

`verify_fixed.py` is an explicit test harness; production Bake never falls back
to it. Machine-readable measurements and the compact validation evidence are
in [`results.json`](results.json). Raw generated code and full diagnostics are
under the reproduction output directories.
