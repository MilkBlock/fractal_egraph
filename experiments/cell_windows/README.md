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
default because unfactored window alternatives can exceed the restricted Gurobi
license. An explicit `EGG_LAYOUT_CELL_WINDOWS=1` or supplied analysis catalog is
respected in full; there is no silent candidate cap. This work does not yet
factor those additional MILP alternatives.

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
