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

## Interface-slot normalization (2026-10-03)

Default registration now alpha-renames **only opaque RipenInput indices**,
anchored by ordered lower then upper interfaces. Ordinary primitive literals,
aliases, action/check order, rules and external obligations remain in the key.
The executable preparation and validation replace history/provenance as the
normalized discriminator. Unsupported input retains the old conservative key.

For a single-parent positive constructor composition, an exact sequential
prefix in both source and validation can be factored into a parent Cell and a
suffix. Deletion, subsumption, functions and unsupported schedules do not use
this factorization. A registry regression verifies differently numbered slots
and different parent preparations sharing one suffix after a parent certificate.
No arbitrary reordering of checks or effects is performed.

Run the real pipeline comparison (build the release binary first):

```sh
cargo build --offline --release --bin egg_layout
python3 experiments/cell_egraph/bench.py --rounds 3 --output out/my-cell-comparison
```

`EGG_LAYOUT_CELL_BINDING_NORMALIZATION=0` selects the old key for ablation only.
The default is enabled in ordinary analyze. This does not change tier0 rules,
schedules, or ripen observation limits, and does not skip ripen tasks.

Measured once per variant, original math-microbenchmark.egg, 3 rounds:

| metric | raw | normalized |
|---|---:|---:|
| registered compositions | 85 | 85 |
| distinct contracts | 74 | 73 |
| completed-catalog union certificates | 41 | 41 |
| resulting Cell eclasses | 44 | 44 |
| factored successors (including multi-parent) | 0 | 7 |
| end-to-end seconds | 1.513 | 1.288 |

The unit-to-eclass partition is identical in this run. The timing is a single
screen, not evidence of a stable speedup (ripen also has wall-time limits).
All 85 normalized contracts succeeded. Of the real nodes, 35 have parents and
all are multi-parent. Exact rebasing/residualization succeeded on 7. It uses
actual token correspondences to substitute each parent boundary into child
coordinates, and requires literal inclusion of all rebased parent actions and
checks. Only then are supplied actions/checks removed; residual order, interface
and ordered parent wiring remain in the contract. Missing correspondences,
missing effects, globals and non-positive actions fall back to the full key.
This is **not** a demonstration of maximal semantic merging in Math: final
eclasses remain 44. Arbitrary equivalent embeddings or parent permutations are
not searched, and there is no claim of a globally optimal representation. Detailed results
are in `math3-results.json`. No claim of tier0 memory compression is made.

The measured variants both recorded 129 applications and 0 excluded applications,
with 79 saturated jobs and 6 observed unfinished jobs. The final unit partition
was unchanged. Four registry/normalization tests and the two native integration
tests passed. No parent certificate is inferred from hashes or similar syntax.
