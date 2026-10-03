# Does eggcc produce reusable local cells?

**Verdict: finite local shared-state candidates exist in this workload. This is
not yet a demonstration of production cell compression or a general reuse proof.**

This is a standalone probe using the actual local egglog engine, all declarations
and original rules from eggcc's exported `fact.bril`, and its actual optimizer
schedule. It bypasses the production export capability gate; it does not replace
rules with a toy optimizer or modify the production pipeline.

## Selection and controls

The earlier run prepared 620 composition entries. Of these, 476 contain the
`initialization` rule that constructs the entire input program. They are excluded
from this *local-cell* experiment; their usefulness at a different granularity
is not assessed. All remaining **144 entries** are tested, without ranking or
sampling. This is still a selected subset of the original 2,060 jobs, not a claim
about all eggcc traces.

Input initialization and optimization are separate schedules in the actual source.
Re-running initialization inside every cell would inject 58 original program rows,
including both `main` and `fact`. The probe runs only the optimizer schedule and
repeats it until a complete pass reports no changes. The unchanged schedule itself
contains eggcc's original repeats, saturations, and phase ordering. This is a fixed
point relative to that schedule, not to every declared ruleset.

An empty-context control reaches 3 rows: `TNil`, `TypeList-length`, `PureTypeList`.
These are explicitly accounted for and are not evidence of useful cell inference.
Concrete primitive values are retained. Equality-sort `RipenInput` slots remain
symbolic, as in the validated entries. Native serialization exports only actual
state values, and the diagnostic comparator handles observed Set membership
without imposing member order. No successful final group in this run contains a
container value: the production declaration-wide container gate was blocking even
these container-free snapshots.

## Results

| Metric | 50 ms | 500 ms |
|---|---:|---:|
| Tested entries | 144 | 144 |
| Reached a schedule fixed point | 140 | 140 |
| Unfinished | 4 | 4 |
| Distinct initial states among completed cases | 68 | 68 |
| Distinct final states | 44 | 44 |
| Final groups containing multiple occurrences | 15 | 15 |
| Occurrences in those groups | 111 | 111 |
| Final groups merging genuinely different initial states | 11 | 11 |
| Groups with different recorded boundary bindings | 12 | 12 |
| Net row growth exceeding the empty control's 3 rows | 126 | 126 |

The 200,000-fact limit is unchanged. Observation deadlines classify a case as
unfinished, never saturated or proven infinite. Snapshot comparison retains row
visibility, primitive payloads, typed values, and boundary-slot aliasing, allowing
slot renaming. It does not certify all substitutions or construct production
input/output adapters. Recorded bindings are compared after applying recorded
unions; they are not a claim of physically disjoint graph coverage.

## A concrete shared cell

The largest group contains 26 observed compositions, with 14 distinct recorded
boundary-binding signatures. Its two initial forms are:

```text
A: Get(x, 0), Add-Gets(x, x, 0)
B: Get(x, 0), Single(Get(x, 0)) = SubTuple(x, 0, 1)
```

Both yield the same seven cell-related table rows:

```text
g = Get(x, 0)
s = Single(g)
Add-Gets(x, x, 0)
Add-Gets(s, s, 0)
Get(s, 0) = g
SubTuple(x, 0, 1) = s
SubTuple(s, 0, 1) = s
```

The native snapshot has 11 rows: these 7, one input marker, and the 3 background
rows. Initial snapshots have 3 or 4 rows including the input marker. The rows mix
constructors and analysis relations; they are not all expression e-nodes.
See `getter-state.json` for the exact normalized diagnostic snapshot.

## Boundary challenge using real input terms

From the original initialization graph, extract every distinct operand of its
`Get` nodes: two `Arg` terms, two `Call` terms, and an `If` term. Union each with
the getter cell's input. Compare:

- warm: start from the locally closed cell, inject the real term, continue;
- fresh: start from the original unsaturated entry, inject the same term, run.

Four cases complete in both paths, and all four final snapshots are isomorphic.
Warm continuation performs **12 fewer logical matches and 7 fewer inserted-row
receipts per case**. This is a finite checkpoint-reuse witness, not an end-to-end
planner or storage optimization. Timings have additional native query-cache
warmth differences and must not be quoted as a cell speedup.

The concrete extension is substantial: final snapshots grow from 11 rows to
121, 121, 202, and 143. The `If` context remains unfinished at both observation
limits. A small closed cell can be reused, but concrete boundary information can
enable much more work; permanent closure after arbitrary grafting is not shown.

## Why the four If cases keep growing

All four unfinished symbolic cases are rooted in `If`. The `swap-if` rule adds
`If(Not(pred), inputs, else, then)`; repeated optimizer passes keep producing
longer `Not` chains and associated type facts. At 500 ms the cases have roughly
1,200–1,400 rows. An explicit attribution ablation removes only `swap-if` from the
schedule, retaining its declarations and all other phases. All four then reach
fixed points of **18, 18, 18, and 24 rows**.

These ablation results are **not** saturation certificates for the original
schedule. The original source deliberately executes a finite optimizer schedule;
repeating that schedule to seek a local fixed point is a stronger experiment.
No formal theorem of nontermination is claimed.

## What this establishes—and what it does not

There is positive evidence for small shared closures and concrete continuation
from them. The earlier zero-cell report did not establish their absence. There
is also a specific expanding family needing schedule-aware or recursive handling.

There is no RSS compression measurement here, no replacement of native facts,
and no general proof that a symbolic cell is safe for every context. Entries can
share history and overlap in tier0; 111 occurrences cannot be read as 111 disjoint
memory regions. Cell selection, interface adapters, and runtime storage costs
remain to be measured separately.

## Reproduce offline

Use the saved analysis directory generated by `experiments/multiple_datatypes/run.py`.
Generated full reports and state groups stay outside Git; only compact results
and one illustrative state are checked in.

```sh
cargo build --release --offline --example eggcc_cell_probe
cargo test --offline --example eggcc_cell_probe
target/release/examples/eggcc_cell_probe /tmp/eggcc-set-final/live /tmp/cell-short 50
target/release/examples/eggcc_cell_probe /tmp/eggcc-set-final/live /tmp/cell-long 500
python3 experiments/eggcc_cell_probe/summarize.py \
  /tmp/eggcc-set-final/live /tmp/cell-short /tmp/cell-long /tmp/cell-summary
```

Use new output directories for each run. `results.json` pins the exported source
by SHA-256. The prior trace was generated from eggcc revision
`ae80f82a403c1209fb084b115ee89470c7e66d0e`; the engine is the local modified runtime,
not the pristine baseline. Tests ensure that Set member order is irrelevant while
membership and primitive values are not erased.
