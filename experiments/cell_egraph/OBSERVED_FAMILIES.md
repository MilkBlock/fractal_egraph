# Fractal candidates from analyze evidence only

Ordinary `egg_layout analyze` now records observed re-entry families in its native
RipSatCell egraph and in each emitted snapshot. The old numeric AST experiment is
an expression-lowering test, not the discovery frontend for these families.

```egg
(datatype CellFamily (ObservedFamily i64))
(relation ObservedFamilyMember (CellFamily Cell))
```

The membership relation is actually inserted into the native Cell egraph.
`cell-egraph.json.repetition_families.membership_rows` reports its stored row count.
Membership never invokes `union`; neither unfinished ripen nor a recurring pattern
becomes an equality certificate. Existing exact closure certificates are unchanged.

## Evidence contract

`native_cell_families.rs` receives only `Captured` analyze records and observed
unit membership. It has no file/network input, fresh matching/ripen API, or
whole-source constructor call-graph scan. Only rules referenced by captured,
state-changing applications contribute semantic descriptors. Their condition ASTs,
typed input/output roles and RHS field expressions explain observed bindings;
they do not introduce new potential edges.

An edge requires an actual `Port::Parent` with a read-row input pointing to a
produced-row output of the stated parent. The versioned fact token must be exactly
equal. Numeric equality alone, shared eclasses, context-history edges, chronology,
and a repeated rule name cannot establish that edge. Updated state evidence,
union effects, external ports, all other parent slots, and alias patterns remain
in the application contract. Numeric samples retain their concrete literals in
the evidence; rule expressions retain constants and conditions. No curve fitting
or arithmetic generalization from samples is performed.

Records with no committed insertion/union/update are not state transitions.
Visibility-changing actions are currently excluded because this family adapter
does not have their complete lineage; they are counted as unsupported.
Scope resets do not connect through the new discovery layer. Missing producer
evidence remains missing; it is not reconstructed by searching the final graph.
Analyze's own capture exclusions therefore also bound this discovery's coverage.

## Detection and representation

1. Intern exact application contracts and typed producer binding contracts.
2. For each new state-changing event whose contract has occurred before, follow
   only captured producer ancestry to previous occurrences of that same contract.
3. Stop a branch at a matching previous occurrence. Keep one shortest provenance
   path per reached start/end, retaining every step's complete parent/externals
   contract. This does not enumerate all paths or all possible subgraphs.
4. Intern the cyclic sequence of binding-contract IDs up to rotation. Rotation
   changes the displayed entry point; it does not commute or reorder operations.
5. Attach the actual record/event paths and registered units rooted at those
   applications to the family. A larger unit does not become a member merely
   because it contains an earlier participating application.
   Preserve different binding positions as different cycles. Shared branches are
   retained in the underlying dependency DAG, without a Cartesian product of paths.

`status: observed-reentry` means finite, connected re-entry has occurred. It does
not prove repeatability for all parameters, termination, infinitude, minimal
trigger state, or a parameterized saturated Cell equivalence. Multi-parent effects
can be inspected, but this implementation does not synthesize a whole branching
recurrence from arbitrary overlapping motifs. It deliberately preserves that gap.

With V captured state-changing events and E producer links, one ancestor search
uses O(V+E) time and O(V) temporary memory in the worst case. Across all events the
worst case is O(V(V+E)), plus emitted path size. Newly seen contracts skip ancestry
search. This is a correctness-first detector; no hidden Top-K, depth budget, or
speculative pairing has been introduced. Large histories need further incremental
indexing before claiming cheap complete discovery.

## Actual example and negative controls

```sh
cargo build --offline --release --bin egg_layout
target/release/egg_layout analyze \
  --source experiments/cell_egraph/observed-families.egg \
  --output out/analyze-cell-families --save-history
python3 tools/egglog_debugger/server.py --port 8080 --binary target/release/egg_layout
```

Open the local standalone viewer:

`http://127.0.0.1:8080/cell-viewer.html?layer_run=out/analyze-cell-families`

Purple family nodes link to existing construction nodes. Clicking one shows its
actual event paths, typed values, expressions and external obligations. Clicking
a member opens that Cell's existing initial/observed/saturated graph. Round
selection reads only that round's family data; old snapshots never borrow final
families. Primitive-bearing Cells may have observed saturated graphs without an
exact sharing certificate; this distinction remains visible.

The example executes Fib(11) and an A/B recursion. It also declares an uncalled
increasing recursion named `never-executed`: that rule must never appear in family
evidence. A shallower Fib(7) run did not establish a repeated exact binding/effect
contract along a captured path; the detector does not fill that gap using the
source's obvious recursion. Fib(11) supplies a witnessed re-entry.

Checks include native analyze, deleting the input source and replaying saved
history, unchanged contract/family evidence after replay, disconnected type-level
cycles, DABCABC, distinct branching binding positions, empty histories, unchanged
Cell equality behavior, and viewer/API snapshot isolation.

```sh
cargo test --offline --release --lib cell_egraph::families
cargo test --offline --release --test analyze_cell_families --test cell_egraph_pipeline
node tools/egglog_debugger/test_cell_egraph.mjs
python3 -m unittest discover -s tools/egglog_debugger -p test_cell_egraph.py
```
