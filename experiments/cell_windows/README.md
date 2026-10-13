# Middle-history Cells and reuse evidence

This runs in the actual native `egg_layout analyze` pipeline. It is not a GPU
prototype, an all-path matrix algorithm, or a new semantic equality test.

## Discovery and admission

For each witnessed mutation, retain the normal prefix and additionally register:

1. The application alone: its required external facts become the entry contract.
2. The application and **all its actual immediate producers**: one complete
   consumer neighborhood, including fan-in. Do not enumerate parent subsets.

These are semantic bases, not a configurable history-length/beam budget. A
window cuts provenance, not prerequisites. Existing replay preparation lifts
outside inputs and checks equalities, staged injections and updates. A failed
replay is rejected; an unfinished ripen is never a saturated Cell.

Windows with the same selected history members reuse one entry. Other windows
reuse existing preparation/session caches and enter the **same** completed exact
closure catalog as rooted compositions. Certified equality admits them into the
same Cell e-class. No union is inferred just from overlapping history or equal
rule names. `container` records where a window was read; it is not a replay
parent, so it cannot pull the discarded prefix back into the window.

Windows do not enter the original event-state or port-frontier indexes. Thus
they do not recursively multiply the original prefix discovery. At most two
window descriptions are formed per mutation; no-effect confirmations add none.
This is not complete discovery of all possible DAG windows or all path lengths.

## Run and inspect

```sh
cargo run --release --bin egg_layout -- analyze \
  --source experiments/cell_windows/chain.egg --output out/cell-window-demo/windows
python3 tools/egglog_debugger/server.py --port 8080
```

Open `http://127.0.0.1:8080/cell-viewer.html?layer_run=out/cell-window-demo/windows`.
The reuse table selects the existing Cell inspector; the same table is also in
the regular debugger page. Per-round snapshots carry the same evidence.
`cell-reuse.json` contains the machine-readable metrics.

Use `EGG_LAYOUT_CELL_WINDOWS=0` for the rooted-only ablation. Standalone analyze
enables windows by default. Automatic **spine Bake** retains rooted capture by
default for continuity with prior compiler benchmarks. SCIP is now the default
solver. Exactly equal typed spine representations share choices while all
history paths remain obligations and validation cases. An explicit
`EGG_LAYOUT_CELL_WINDOWS=1` or supplied analysis catalog is respected in full;
there is no silent candidate cap. Full-window measurements and reproduction
commands are in [the solver report](../cell_spine/scip/README.md).

For the one-run-per-mode comparison:

```sh
python3 experiments/cell_windows/compare.py --output out/cell-window-comparison
# Add --source experiments/cell_spine/math_tiny/source.egg for the math subset.
```

Membership fingerprints only locate possible duplicate history entries; exact
membership is checked on every hit. Prefix membership vectors are not copied
into the index. Fact evidence is stored once per event, and coverage follows the
existing state DAG with a visited set. It does not expand every derivation path.

## What the table means

- `registered_constructions`: admitted replay entries, not all DAG paths.
- `interface_instances`: distinct ordered typed input/output token signatures
  among certified entries. Versions remain distinct. Neither symmetry nor later
  union aliases are quotiented, so this is a conservative history-binding count,
  not an exact physical live-instance count. Incomplete interfaces are reported
  separately, not assigned fabricated keys.
- `historical_fact_coverage` / `historical_effect_fact_coverage`: union of
  versioned witnessed fact IDs / produced fact IDs. These are not reconstructed
  full tier-0 e-node coverage. Union events have a separate counter.
- `overlapping_fact_references`: sum of per-binding fact coverage minus its
  union. Alternative proofs of one binding are combined before this calculation.
  Coverage of **different** Cells can also overlap and must not be summed.
- `external_fact_coverage`: distinct historical fact requirements supplied from
  outside the selected windows. This is an observable interface obligation, not
  a prediction of future Join work.
- `isolated_description_gain`: a deliberately limited reference model:
  `n * B - B - sum(interface_fields + 1)`, where `B` counts non-RipenInput closure
  row fields (arguments plus result), and `n` is the interface-binding count.
  The extra field is a template reference. This assumes separate body copies;
  it excludes cross-instance hashcons/overlap, indexes, retired marker storage,
  allocation and maintenance. A positive score is **not measured memory savings**.

Only entries in the completed exact catalog are ranked. Pending, rejected and
unfinished entries remain in the analysis job report and receive no reuse
guarantee. Only successfully registered entries have nodes in the Cell graph.
Sorting is by interface-binding count, then distinct observed effects; it is
display order, not a frequency threshold or candidate-pruning heuristic.

## Small native checks (2026-10-13)

One run per mode, same local kernel, graph rendering disabled, one analysis
worker. No timing average or statistical speed claim:

| Input | Mode | Entries | Certified closure kinds | Window readings | Wall time |
|---|---|---:|---:|---:|---:|
| `chain.egg` | rooted | 10 | 2 | 0 | 0.086 s |
| `chain.egg` | windows | 21 | 4 | 17 | 0.105 s |
| math tiny | rooted | 6 | 3 | 0 | 0.520 s |
| math tiny | windows | 9 | 3 | 9 | 0.051 s |

The math input is `experiments/cell_spine/math_tiny/source.egg`. Cold process/file
cache effects and timing noise dominate these short measurements; the second
pair does **not** establish a speedup. All entries saturated in these fixtures.
The chain's `B/C/Done` closure has six registered constructions but only three
interface bindings. Four of those entries come from new windows. The math case
adds three entries and no new closure kind: all three reuse existing states.

Tests cover middle-prefix removal, complete multi-producer windows, ordinary
state discovery, online/offline consistency, occurrence/overlap deduplication,
unrated unfinished states, and browser selection of the new statistics.

## Bake/runtime comparison: windows are not recursive small-Cell recombination

Run the same source through fresh rooted-only and window-enabled Bake, then
benchmark its generated programs with the existing native CI harness:

```sh
out/cell-spine-venv/bin/python experiments/cell_windows/spine_bench.py \
  --output out/cell-windows-spine-bench --copies 128
```

It defaults to math tiny and the eggcc If subset. `--cases math_tiny` is the
quick smoke comparison; `boundary` is also available. It uses existing release
binaries, SCIP and the default projected layout pricer. Both discovery modes
use 50 ms / 200K facts per ripen, four steps per sweep, one analysis/query/Rayon
thread, and no graph rendering. There are no extra candidate caps. Sizes and
rule-match diagnostics run separately from timing. The original native mode
uses the same local kernel, not pristine egglog. Every mode is measured once.

Measured 2026-10-13; [`spine_bench_results.json`](spine_bench_results.json) preserves
the versions, binary hashes, phases, sizes and input-spine/raw-ingress results:

| Input | Discovery | Entries | Certified Cell kinds | Bake s | Library KiB | Physical rows, 128 copies | Input-spine Run ms |
|---|---|---:|---:|---:|---:|---:|---:|
| math tiny | rooted | 6 | 3 | 0.630 | 4.90 | 1,152 | 0.342 |
| math tiny | windows | 9 | 3 | 0.639 | 4.90 | 1,152 | 0.303 |
| eggcc If | rooted | 8 | 3 | 115.273 | 132.86 | 10,247 | 76.280 |
| eggcc If | windows | 10 | 5 | 124.977 | 137.73 | 10,247 | 75.641 |

Expanded nodes/classes match native execution in both modes: **1,664 / 896**
for math, **6,664 / 5,640** for If. These are actual exported counts. Seed/history
cases additionally pass full native closure checks; replicated workloads use
count comparison. The three generated math `.egg` files are byte-identical
between discovery modes. If has 55 vs 59 emitted rules; its library grows by
4,986 bytes, without reducing physical rows. Single-run sub-millisecond Run
differences do not establish an inference speedup.

The missing connection is explicit in the audit:

- Rooted If already has two selected multi-parent implementations, lowered to
  **50 multi-marker merge rules**. Both modes record **512 complete matches**
  of these rules, in both raw-ingress and direct-input-spine runs. Matches are
  not new Cell insertions. Distinct marker patterns can belong to one family.
- Both modes have **zero observed edges using a newly created window as parent**.
  No chosen parent representation uses a new window. The two extra If closure
  families participate in **zero** multi-marker merge matches.
- Thus this measures admission of middle-history Cells plus existing
  composition, not a completed unrestricted small-Cell + small-Cell algorithm.
  Windows are not automatically registered into the event/port frontier for
  subsequent consumers. This missing discovery connection must be completed
  before attributing benefits to recursive window recombination.

The full output includes `cell-audit.json` per variant with the exact merge-rule
names and counts, `phases.md` for installation/input/Run/export costs, and per-table
rows in `report.json`. Overall runtime remains worse than native on this If
subset; the added windows do not reverse that result. Objective values from
different catalogs are not compared as if their transition obligations matched.

## Literature and the specific connection

These motivate the reporting model; this implementation does not import their
mining heuristics or claim their completeness/optimality guarantees.

1. Mistry, Roy, Sudarshan, Ramamritham, **Materialized View Selection and
   Maintenance Using Multi-Query Optimization**, SIGMOD 2001.
   [Author PDF](https://www.cse.iitb.ac.in/infolab/Data/Publications/sudarshan/viewmaint-sigmod01.pdf).
   Reuse benefit must include materialization and maintenance, and choices
   interact through shared subexpressions. This motivates reporting boundaries
   and overlap alongside frequency, rather than adding isolated gains.
2. Vreeken, van Leeuwen, Siebes, **Krimp: mining itemsets that compress**, DMKD
   2011. [Paper](https://link.springer.com/article/10.1007/s10618-010-0202-x).
   MDL charges the dictionary as well as encoded uses. Frequency alone does not
   justify a pattern. Our reference formula is a much simpler structural model,
   not Krimp's code lengths or cover algorithm.
3. Cao et al., **babble: Learning Better Abstractions with E-Graphs and
   Anti-Unification**, POPL 2023. [Author PDF](https://cseweb.ucsd.edu/~npolikarpova/publications/popl23.pdf).
   Equivalent syntactic forms can share an abstraction. Here exact ripen catalog
   equality supplies the semantic grouping; babble is not reconnected.
4. Green, Karvounarakis, Tannen, **Provenance Semirings**, PODS 2007.
   [Author PDF](https://www.cs.ucdavis.edu/~green/papers/pods07.pdf).
   Derivation multiplicity and set-valued provenance answer different questions.
   We separate construction count, interface bindings and distinct fact evidence.
5. Goodman, **Semiring Parsing**, Computational Linguistics 1999.
   [Paper](https://aclanthology.org/J99-4004/).
   Packed derivations can support different counting algebras. This motivates a
   future path counter, not replacing actual multi-input dependencies with
   ordinary paths or treating registered alternatives as all-path counts.
