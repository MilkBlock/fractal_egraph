# Original minimal adaptive Bake case

This fixture uses the **actual egglog runtime**:

```text
Bake: A(x) -> B(x)
B(x) and Extra(x,y) -> C(x,y)
C(x,y) -> D(x,y)
```

The two Extra rows share x but have distinct y values. Both legal bindings and
both final C/D equalities must survive. `graft-fresh.egg` substitutes new concrete
values while retaining the same source rules and relative binding shape.

```sh
cargo build --release --offline
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout analyze \
  --source experiments/reuse_feedback/minimal/seed.egg --output out/minimal-seed
target/release/egg_layout reuse-bake out/minimal-library.json out/minimal-seed
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout reuse-cycle \
  experiments/reuse_feedback/minimal/graft.egg out/minimal-library.json out/minimal-first
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout reuse-cycle \
  experiments/reuse_feedback/minimal/graft-fresh.egg out/minimal-first/reuse-library.json out/minimal-second
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout reuse-cycle \
  experiments/reuse_feedback/minimal/graft-fresh.egg out/minimal-second/reuse-library.json out/minimal-third
cargo test --offline --test reuse_minimal_cycle --test reuse_fragment
```

All output paths must be new. `result.json` records:

| Cycle | Summary reuses | New enclosures | Bodies / enclosures / entries |
|---:|---:|---:|---:|
| First learning | 0 | 3 | 4 / 4 / 4 |
| Fresh binding | 2 | 0 | 4 / 4 / 4 |
| Repeated binding | 2 | 0 | 4 / 4 / 4 |

The first cycle has no registered summary view: source history and its actual
committed effects drive coarse composition/ripen learning. The summary observer
therefore does **not** establish a closed-instance break count for this first
cycle. The older `../graft.egg` fixture separately exercises observed summary
reuse followed by a committed external graft. Do not label missing observation
as a measured absence of breaks.

The learned body contains more than the consumer's two LHS atoms. The planner now
validates that positive fragment against the visible baked rows with typed,
arity-aware conjunctive matching. Repeated variables must map to the same body
slot; distinct variables may alias. Marker/hidden rows are excluded. The result
is a query-reuse opportunity, not closure or trigger equivalence. The finite-body
witness selects a candidate; correctness on new bindings comes from materializing
the original LHS conjunction over current native facts, not from generalizing
that witness into a derivation rule. Validation is
cached by normalized fragment shape, never by concrete runtime binding. This
fixture uses one validation-cache entry and one summary for its two bindings.

For B(x) + Extra(x,y), scanning B first leaves Extra's y lookup key unbound.
The planner can instead start from Extra(x,y), then perform the keyed B(x) lookup.
All later lookup keys must already be bound (or literal), so the summary has at
most the anchor table's row count. Independent Cartesian fragments still fall
back. Only source LHS candidates are checked; no library-pair enumeration or
arbitrary fragment generation is introduced. Exact containment can nevertheless
be expensive on large symmetric bodies; no constant-time claim is made.

The integration test runs independent native references, compares all user-table
final state, and checks the source occurrence/layer DAG. Independent applications
can execute in a different order after Join reordering, so the latter comparison
uses a bijection preserving parent edges, layer membership and DAG sharing rather
than raw run-local IDs. Plain native capture and the planned capture still admit
the same structural definitions; unfinished bodies remain ineligible for Bake.
