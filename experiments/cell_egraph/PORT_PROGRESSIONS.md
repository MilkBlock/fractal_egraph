# Closure-backed port progressions

This is part of ordinary native `analyze`, not a source-parser demo. It uses two
existing evidence sources: captured versioned producer reads and the exact,
completed RipSat catalog. It does not create new matches or re-ripen candidates.

The new result is a finite **indexed view** inferred from interaction wiring.
It is not a predefined Array constructor, an arbitrary-length recurrence proof,
a memory-layout guarantee, or a complete polyhedral compiler.

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
python3 -m unittest discover -s tools/egglog_debugger -p test_cell_egraph.py
```
