# RipSat-semantic Cell egraph: congruence experiment

Run from the repository root:

```sh
EGG_LAYOUT_GRAPHS=0 cargo test --offline --test cell_egraph_normalization -- --nocapture
```

`ir.egg` represents compositions as `CoarseApply(parent, rule, binding)`.
A graph has a fixed rule environment. Binding contains ordered interface ports
and external-obligation identities. These IDs are canonical only within this
experiment; a general interface remapping compiler is not implemented here.

The test runs the real native ripen runner twice, using the finite rule
`A(x) -> B(x)` with union semantics. The left coarse composition injects
`Input(0)` then `A(input)`; the right injects Input, A and B together. Their
initial states differ. Their completed closures are compared by the existing
exact state comparator, including named input/root ports and environment.
Only successful comparison authorizes the Cell parent union.

Successors and grandchildren are constructed BEFORE that union. Native egglog
rebuild then merges both pairs by congruence, with no Cell rewrite rules and
no ripening of either successor. Different connection ports, external
obligations and rule identities do not merge. A swapped closure interface and
a different rule environment are also rejected by certificate comparison.

Verified result (2026-10-03):

- native ripens: 2
- certified parent unions: 1
- automatically merged successor pairs: 2
- successor ripens: 0
- negative controls: 5, plus initially different coarse states
- test: 1 passed (0.03s test execution, excluding build; not a performance claim)

Scope: standalone normalization integration test using actual native egglog
and ripen, not a runtime packing or memory benchmark. The source compositions
and their Cell terms are paired explicitly by the fixture. Production automatic
composition registration, parameterized proof generalization, interface
canonicalization and unfinished-state certificates are not implemented by this
experiment. It does not claim equivalence for arbitrary bindings or nonmonotone
rule environments. Existing unrelated working-tree changes are not part of this
commit; rerun against the complete current checkout.

## Automatic analyze integration

The ordinary `analyze --source INPUT --output OUT` path now constructs a native
Cell egraph while preparing observed CS units. No extra command or environment
flag is needed. `OUT/cell-egraph.json` records unit-to-eclass membership and
parent edges; the same data appears as `cell_egraph` in every saturated-rule-
composition report. Job `cell_unit` links the entry to this registry.

`native_cell_egraph.rs` interns preparation contracts and registers
`CoarseApply(parent-list, contract-id)`. A contract preserves the typed reuse
recipe, interface, prepared source, validation and parent coordinate-contract
IDs. Parent contracts are interned IDs, not recursively copied descriptions.
CS units rejected before preparation succeeds are not registered. If a parent
has no registered contract, the child retains its full standalone preparation;
only known parents become edges.

After the existing exact catalog refresh, completed entries in one certified
family are unioned in the Cell egraph. Family IDs are interpreted only within
that refresh. Previously admitted pairs are not submitted again. Weak dominate,
reentry similarity and unfinished ripen do not authorize unions. Native rebuild
runs after certificate unions. This integration currently does not skip ripen
jobs or change tier0 execution.

Important limitation: the full preparation/parent-coordinate contract is a
conservative node key. It does not yet erase history differences or derive a
minimal interface-relative graft. Consequently equivalent parents do not ensure
that independently prepared successor contracts hashcons. The standalone test
above proves congruence for identical bindings; the online test proves automatic
registration and certified merging, not maximal successor normalization.

Checks:

```sh
cargo test --offline --lib catalog_refresh_is_idempotent
EGG_LAYOUT_GRAPHS=0 cargo test --offline --test cell_egraph_normalization --test cell_egraph_pipeline
```

The online fixture registers 4 real CS units, obtains 1 certified union, and
reports 3 Cell eclasses. Registry tests cover multi-parent edges, distinct
external obligations, unknown parents and refresh-local family renumbering.
