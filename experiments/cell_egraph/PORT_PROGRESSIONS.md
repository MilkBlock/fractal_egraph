# Closure-backed port progressions

This is part of ordinary native `analyze`, not a source-parser demo. It uses two
existing evidence sources: captured versioned producer reads and the exact,
completed RipSat catalog. It does not create new matches or re-ripen candidates.

The new result is a finite **indexed view** inferred from interaction wiring.
It is not a predefined Array constructor, an arbitrary-length recurrence proof,
a memory-layout guarantee, or a complete polyhedral compiler.

## What a user can rely on

This detector recognizes **two-read, fixed-width, complete-overlap translations**.
It now emits `discovery.contract` and one `discovery.outcomes` entry for **every
Captured record in this snapshot**, including records outside that class. The
ordered gates are: committed effect, supported visibility, exactly two reads,
completed catalog, verified producers/interfaces, equal source closures with at
least two ports, complete translation, fully accounted target ports, retained
source bodies, and a consistent nonbranching acyclic component.

If all gates pass, the interaction receives an `indexed` outcome and appears in
the finite coordinate view. Otherwise its first failing gate is recorded; later
gates are **not checked**, not silently considered failures. Branching/cyclic
overlaps remain in their family with evidence, but receive no linear coordinates.
Every outcome has a stable reason, category, record/event references, a target
unit when registered, and a next-experiment explanation. Two-read source checks
also identify read slots, fact tokens, producer records and source units. No
additional matching, ripening, whole-graph search or candidate limit is involved.

The accounting identity is:

```text
captured records = recognized + missing evidence + outside scope + incompatible
```

This is **not** completeness over all tier0 matches, all graph patterns, or all
possible producer choices. Capture exclusions, apply-entry registration and exact
closure certification still bound the evidence. No coverage of unrecorded
matches is inferred from this accounting. Nor does this contract establish Multi-ruleset schedule equivalence: its
closure checks are relative to the catalog's actual rule environment.

The viewer's “端口推进发现” panel presents the contract, positive/negative examples,
reason counts and every record with links back to source/target Cells. It defaults
to two-read interactions; all records are available. Pagination is display-only.
Old snapshots without an audit explicitly say so and never borrow final results.

### Designing facts

Start with three instances `L(a,b), R(b,c), L(c,d)` of an equivalent closed body.
Let the actual consumer read neighboring `Pair` rows and change state twice. The
small test then exercises the same positional connection twice; it does not
require teaching the detector an Array constructor or a numeric index.

Compare these controls, generated from the same fixture in `analyze_cell_ports`:

| Change | Expected meaning |
|---|---|
| Original shuffled L/R fixture | Two indexed interactions; three windows, four positions |
| Remove `Pair -> L` | Interactions still occur, but source closures differ; no shared port family |
| Replace `R(17,2)` with `R(51,64)` | Three independent instances; no two-read consumer, no invented adjacency |
| Add `R(17,99)` | Branching evidence retained; the component is not labeled a linear array |

The advice distinguishes missing experiment evidence from unsupported machinery.
It never recommends erasing a real external input, dropping a branch, or adding
arbitrary facts merely to force acceptance. More rounds alone do not repair a
missing provenance channel or an unsupported interface.

## Example

`port-progression.egg` declares Atom, L, R, Pair and Out. There is no array, index,
successor, or coordinate constructor. Its inputs are deliberately out of order:

```text
L(Atom(2),  Atom(91))
L(Atom(83), Atom(17))
R(Atom(17), Atom(2))
```

L and R can both reach the same complete local closure containing L, R and Pair.
A real consumer reads `Pair(a,b)` and `Pair(b,c)` and produces `Out(a,b,c)`.
The source only provides local contexts; no source list defines their order.

Analyze certifies equivalent L/R bodies with fixed RipenInput port coordinates.
Its actual consumer reads then establish two overlaps. The inferred view is:

```text
window i=0: X[0], X[1]
window i=1: X[1], X[2]
window i=2: X[2], X[3]

interaction i=0: window 0 + window 1 -> target(X[0], X[1], X[2])
interaction i=1: window 1 + window 2 -> target(X[1], X[2], X[3])
```

The first observed consumer is at coordinate 1, and the second is at coordinate
0. Thus coordinates do not follow match time, numeric Atom values, or seed order.
Displayed element identities are analyze's typed tokens; the detector does not
query the original graph to recover erased Atom labels.

## Evidence and algorithm

1. Use only existing `apply_entry` views that have completed exact catalog
   certificates. Unfinished and observation-only bodies cannot provide this gate.
2. Resolve each view's exported RipenInput terms into the catalog representative's
   port roles. Literal marker indices and types fix the frame. Keep the concrete
   input-token binding beside it; different histories can denote the same instance.
3. For an actual consumer with two recorded reads, require both versioned fact
   tokens to point to the stated producer's committed row. Validate its argument
   columns and the corresponding row in the certified body. No instance-pair
   enumeration and no native Join are performed by this detector.
4. Group using source **closure identity**, target closure identity, read-column
   roles, overlap and source-to-target port maps. Producer rule names are evidence
   only. This is where closure equality removes differences between histories.
   Deterministically transport every visible non-parameter row of both source
   bodies into the target, preserving their equalities. Reject a target that only
   has the right ports but omits internal facts. Parameter-marker rows are renamed
   through explicit bindings. Additional target unions may merge source classes;
   the mapping need not be injective. A dependency worklist performs this check
   through indexed constructor lookups, without subgraph search. The proof is
   shared across occurrences of the same port contract.
5. Accept a translation d only when the overlap is exactly
   `{(j+d,j) | 0 <= j < width-d}`, with `0 < d < width`. This is an exact check on
   port roles, not a numerical regression on sample values.
6. Build the observed overlap graph. A nonbranching acyclic component gets a
   coordinate origin and step d. Reconstruct its element positions and verify all
   overlapping positions agree. Branching, cyclic or inconsistent components keep
   their diagnostics and get no linear array coordinates.

All parent reads must be accounted for, and all target interface parameters must
come from those sources. The current gate rejects extra/unresolved parameters.
It does not hide coarse external obligations to make a progression look smooth.
Same-valued elements may occupy different coordinates when those positions remain
distinguishable in the evidence. It does not reconstruct multiplicity already
erased by hashcons, nor guess ambiguous aliased input roles.

The report contains source accesses `d*i+j` and `d*i+d+j`, and the induced target
parameter accesses. Multiple offsets for one target parameter explicitly retain
the observed alias requirement. These describe the **Cell transition's parameter
accesses**, not a claim of minimal semantic dependencies for the original program.
The exact observed integer domain is included; it is not extended to unseen N.
Each connected component has its own snapshot-local origin. New evidence may
prepend a window and move that origin; these are not stable runtime addresses.

## Verification and limits

The example has three equivalent source instances and two witnessed interactions.
Two different producer-rule arrangements normalize to one port family, covering
three windows and four positions. The older event-contract re-entry detector finds
no recurrence family in this example: the new evidence is a closed-piece overlap
relation, not a sequential loop of the same apply contract.

A negative control removes `Pair -> L`. Both overlap applications still occur,
but L- and R-originated stored closures now differ. The detector refuses to merge
them into this port family. This is a correctness control, not a speed ablation;
it also shows the conservative full-closure requirement. Different closed bodies
that expose equivalent smaller interfaces are not yet unified by this method.

Tests also delete the source before history replay, check all derived parameter
accesses against captured bindings, and cover incomplete overlaps, reversed event
order, repeated values at different positions, branches, cycles and contradictions.
The previous analyze/re-entry tests continue to run.

Only two-read complete-overlap translations over fixed lower input roles are
currently recognized. Arbitrary internal ports, n-way consumers, multidimensional
lattices, variable-size interfaces, symbolic numeric guards and arbitrary affine
maps remain outside this implementation. The display calls them closure-backed
port families, not certified universal FractalRules.

Catalog rows are indexed once per report generation and never re-exported here.
Unchanged record/window/catalog versions reuse the report. The index needs O(R)
space for R exported row-argument entries. Coordinate traversal uses linear graph
work apart from ordered-map lookups and the emitted evidence. No Top-K or hidden
search/depth budget is added.

## Reproduce and view

Generate all four native cases (including round snapshots and saved replay data):

```sh
cargo build --offline --release --bin egg_layout
python3 experiments/cell_egraph/discovery_examples.py --output out/discovery-contract-verified
python3 tools/egglog_debugger/server.py --port 8080 --binary target/release/egg_layout
```

Open `http://127.0.0.1:8080/cell-viewer.html?layer_run=out/discovery-contract-verified/positive`.
Use the directory selector for `different-closures`, `disconnected`, and `branch`.
The example runner requires a fresh output directory and keeps each exact input.
The two browser regressions accept these output paths as arguments:

```sh
node tools/egglog_debugger/test_cell_ports_browser.cjs out/discovery-contract-verified/positive
node tools/egglog_debugger/test_cell_discovery_browser.cjs out/discovery-contract-verified
```

To run just the original example:

```sh
cargo build --offline --release --bin egg_layout
target/release/egg_layout analyze \
  --source experiments/cell_egraph/port-progression.egg \
  --output out/closure-port-progressions --save-history
python3 tools/egglog_debugger/server.py --port 8080 --binary target/release/egg_layout
```

Open `http://127.0.0.1:8080/cell-viewer.html?layer_run=out/closure-port-progressions`.
The cyan P node opens its port contract, actual event evidence and coordinate
table. “查看端口推进图” shows the instance overlaps; window nodes open the original
Cell body, and event diamonds open their actual rule. The original full Cell graph
is retained. Round snapshots never borrow later certificates or coordinates.

```sh
cargo test --offline --release --lib cell_egraph
cargo test --offline --release --test analyze_cell_ports --test analyze_cell_families --test cell_egraph_pipeline
node tools/egglog_debugger/test_cell_egraph.mjs
node tools/egglog_debugger/test_cell_discovery.mjs
python3 -m unittest discover -s tools/egglog_debugger -p test_cell_egraph.py
```
