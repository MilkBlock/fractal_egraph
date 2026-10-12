# Native runtime ablation of spine and marker selection

The joint structural score is **not a reliable runtime cost model yet**. It
improved the long-spine graft but slowed down the sparse If workload. This
experiment uses real current egglog execution, not a standalone simulator.

All three variants use one frozen Bake catalog per source:

- `old`: frozen old spine and marker choices from `2402786`.
- `marker_only`: keep those spine/implementation choices, optimize markers.
- `joint`: use the joint optimum from `3401f61`.

The long and If cases choose the same spines in all variants; marker-only and
joint consequently emit byte-identical libraries. AC chooses another tied
spine but gets the same score and same measured search work. These cases do not
demonstrate a separate runtime benefit from changing spine selection.

## Single-run measurements

Each row below uses 1,000 fresh instances; shared type/operator constants remain
shared. For If, 100 are matching min/max patterns and 900 are near misses. Each library is frozen; the held-out input is
not baked. Full typed constructor graphs and e-class bijections are verified in
separate processes. The source inputs were also tested independently.

| Input | Mode | Inference ms | Whole execution ms | Peak RSS MiB | Equivalent e-nodes / e-classes |
|---|---|---:|---:|---:|---:|
| 3-input AC | native | 6.187 | 59.319 | 29.66 | 15,000 / 7,000 |
| 3-input AC | old | 7.069 | 63.081 | 30.00 | 15,000 / 7,000 |
| 3-input AC | marker-only | 7.142 | 63.686 | 29.16 | 15,000 / 7,000 |
| 3-input AC | joint | 7.504 | 65.221 | 30.69 | 15,000 / 7,000 |
| Long graft | native | 7.695 | 134.928 | 35.86 | 18,000 / 18,000 |
| Long graft | old | 19.852 | 153.378 | 39.05 | 18,000 / 18,000 |
| Long graft | marker-only | 15.336 | 147.338 | 40.61 | 18,000 / 18,000 |
| Long graft | joint | 15.514 | 147.698 | 39.20 | 18,000 / 18,000 |
| Sparse If | native | 47.654 | 1145.705 | 285.20 | 16,208 / 14,108 |
| Sparse If | old | 50.224 | 1319.327 | 305.16 | 16,208 / 14,108 |
| Sparse If | marker-only | 105.544 | 1272.637 | 306.05 | 16,208 / 14,108 |
| Sparse If | joint | 98.964 | 1269.477 | 297.38 | 16,208 / 14,108 |

Whole execution includes installation, input and inference; these rows stop at
the Cell graph. The CSV separately reports complete export modes. Input parsing
and let creation dominate the If total. Old/joint If installation takes 132.258 /
59.935 ms, while inference gets slower; the smaller total is **not** faster
inference. Byte-identical marker-only/joint programs also show single-run noise.

`native` is ordinary execution on the same current kernel, **not pristine**.
This kernel already has unconditional atomic probe counters. No new engine
instrumentation was introduced, but these are not uninstrumented production
times. No averaging or repeated selection of a fastest result was performed.

## What the search actually did

Existing probe counters are read before and after **inference only**, in
separate diagnostic processes. They exclude installation, input and export.

| Input | Mode | Logical matches | Index handles | Candidate rows across stages |
|---|---|---:|---:|---:|
| AC | native | 18,000 | 14 | 26,000 |
| AC | old / joint | 8,000 | 10,044 | 36,000 |
| Long graft | native | 2,000 | 58,020 | 86,000 |
| Long graft | old | 3,000 | 102,016 | 140,000 |
| Long graft | joint | 3,000 | 70,014 | 96,000 |
| Sparse If | native | 2,100 | 203,516 | 318,200 |
| Sparse If | old | 2,200 | 192,550 | 913,354 |
| Sparse If | joint | 2,200 | 695,414 | 1,276,552 |

Candidate rows aggregate accesses over stages; they are not distinct failed
matches. Index handles are not lookup calls. Logical matches are not committed
mutations. The If single-key lookup count increases from 183,854 to 682,752.

For If, static read terms fell from 1,859 to 711. Even read terms weighted by
successful match frequency fell from 7,600 to 4,900. **Both proxies miss the
increased search work.** These aggregate counters establish that the search
plan's work changed; they do not attribute the increase to a particular rule or
claim that one kind of probe has a constant time cost. Changing weights based
only on successful matches would not solve this problem.

The follow-up in `../if_join_profile/README.md` attributes the regression to two
zero-match inherited rules. Actual stage counters expose 50-by-1,000 attempted
bindings before rejection; restoring a canonical lookup alone does not help.
That experiment adds per-rule plans and opt-in block/stage diagnostics without
changing the production planner or deleting rules from the compiler.

## Storage and code size

The phased harness previously counted raw `Spine-*` constructors but omitted
`Spine-Cano-*`. This experiment fixes that omission. All table rows, including
auxiliary/global-binding rows and subsumed Summary rows, are now also reported.

| Input / 1,000 copies | Raw nodes | Canonical nodes | Active / retired Summaries | All physical table rows |
|---|---:|---:|---:|---:|
| AC, all variants | 5,000 | 3,000 | 1,000 / 2,000 | 12,000 |
| Long graft, all variants | 15,000 | 7,000 | 1,000 / 1,000 | 25,000 |
| If, all variants | 14,006 | 3,201 | 1,900 / 200 | 33,313 |

Marker widths differ, so equal row counts do not mean equal bytes. No duplicate
Summary reduction was obtained from this marker optimization. The old/joint If
libraries have 230,313 / 173,790 bytes but **both have 55 inference rules**.
The long-graft libraries have 10,016 / 9,660 bytes and four inference rules.

## Full seven-input Add/AC boundary

`fixtures/ac_seven.egg` keeps the actual two AC rules, seven numeric inputs,
`run add-ac 7`, and check from the upstream web-demo scope. Only unused
declarations are removed. A **separate** saturation variant is tested; the
original finite schedule is not silently replaced.

Both native executions yield exactly **1,939 e-nodes / 127 e-classes**, with full
graph equality. Finite7 inference took 4.287 ms; saturation took 4.397 ms.

Normal traced analysis of the saturation variant did **not** finish within the
experiment's 60-second external guard. At interruption its observed process
group RSS was 2,216,050,688 bytes (about 2.06 GiB), not a measured peak. It had
written 3,469 entry directories and 510 pairs of initial/trigger state files,
but had not published `analysis.json`. **Gurobi was never reached. There is no
successful seven-input Cell program or compression result from this run.**

The guard stops this measurement, not the algorithm's candidate generation; no
candidate cap or AC-specific compression was added. The compiler still rejects
the original finite-round schedule for symbolic Bake. The separate saturation
fixture passes that static admission check but exposes the earlier analysis
cost. The measured bottleneck is upstream of spine selection.

An older full benchmark smoke test (`tools/bench/test_cell_spine.py`) also
exposed a boundary: compilation of `math_tiny/expanded.egg` exceeded its existing
30-second Bake guard at about 145 MiB observed RSS. It is recorded as unfinished,
not a passed test. This experiment does not raise that guard to hide the cost.

## Reproduce

```sh
cargo build --offline --release --example native_math_baseline

# Generate one shared Bake/catalog/optimum for each input if not already present.
out/cell-spine-venv/bin/python experiments/cell_spine/joint_pricing/run.py \
  --output out/joint-pricing-new --skip-build

out/cell-spine-venv/bin/python tools/bench/spine_ablation.py \
  --bake-root out/joint-pricing-new --output out/spine-ablation-new --copies 1000

out/cell-spine-venv/bin/python experiments/cell_spine/joint_pricing/ac_seven.py \
  --output out/ac-seven-new

out/cell-spine-venv/bin/python -m unittest discover \
  -s tools/bench -p test_spine_ablation.py -v
```

To add only counters to an existing ablation report, use
`spine_ablation.py --output PATH --diagnose`. This preserves original timing
measurements and records a separate diagnostic binary hash. `results.csv` uses
milliseconds for phase columns; `metadata.json` records full-graph checks,
selected paths/markers, code sizes and binary provenance. `ac_seven.json` records
the seven-input success and analysis failure separately.
