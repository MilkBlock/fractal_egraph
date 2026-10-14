# Left/right graft soundness and discovery audit

The probe separates history coverage from actual generated-code correctness.
It executes native egglog capture/ripen and the production spine compiler.
No operator names are special-cased in the fix.

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/graft_diamond/probe.py \
  --output out/my-graft-diamond
cargo test --offline --release --test graft_diamond
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_graft_diamond
```

Native tools must already be built. Each run uses a new output directory.
The tiny histories are saved for inspection; no raw streams are checked in.

## What was reproduced

Start with a base application producing `Add(x,y)`. Independent external
`GateL(x)` and `GateR(y)` enable `Left(x)` and `Right(y)` respectively.

| Observation | Result |
|---|---|
| Left then right, across capture boundaries | Right's producer is base, but its contextual parents also include the left prefix |
| Right then left | Symmetric extra contextual parent |
| Both in the same batch | Neither sibling becomes the other's parent |
| Independent history windows enabled | Both single-side closures exist and pass native validation |
| Window lineage before the graph repair | Independent window was a root, not linked back to base |
| Windows disabled | The later independent branch is absent from this catalog |

The right window from a left-then-right run equals the right-only run's closure;
the left window equals the left-only closure. Both two-sided execution orders
also have identical complete closures. The windows contain no effects from the
omitted side. These are exact state comparisons with typed/literal identities.

An equality guard adds a separate issue: `Add(a,b), GateR(b), a=b -> Right(b)`
cannot run until an earlier rule unions a and b. The actual native match obeys
this. However, the graph does not list the union event as a parent: replay
externalizes its consequence as a self-argument `Add(p0,p0)`, a Gate, and an
appropriate output-alias union. Hence `producer_parts` is not a complete
equality-support certificate, and deleting context edges blindly is unsafe.

## A real executable omission, and its fix

The stronger test initially supplies both `Add(x,y)` and `Add(y,x)` while baking
commutativity plus `Add(a,b), Gate(a) -> Mark(a)`. This makes the marked left and
right instances share one Cell family with different bindings.

Reusing that library with only `Add(x,y)`, followed later by `Gate(y)`, failed
before the fix: native produced 6 constructor rows, the compact version only
5. The missing row was `Mark(y)`; there was no spurious `Mark(x)`. The canonical
spine stored one Add orientation, and the compiler had not provided an interface
for the other orientation already present in the certified closure.

`certified_boundary_cut` now factors a known target through a single parent's
port-visible closure row. Its arguments must be known parent inputs/literals,
its result must be the parent root, and the entire binding must be determined.
Both programs must be in the same constructor/equality environment, checked on
native ASTs. Primitive guards, updates, subsumption, unknown ASTs, and fixed
global ports are not admitted by this new proof path.

For parent seed P, target seed C and binding sigma, the admission check requires:

1. The matched target fact is provided by the parent's certified closure.
2. The target closure contains **every parent seed fact and equality guard**
   under sigma. Sharing one node or just the root value is insufficient.
3. Every remaining target fact/guard is retained explicitly in the consumer.

For this positive fragment, P sigma being contained in Sat(C) implies that
adding the parent's closure introduces no fact outside Sat(C). The visible
closure row plus the remaining seed facts still supplies C. Root value identity
and physical constructor storage remain separate in lowering. Existing native
full-state validation checks the emitted program as before.

Bindings already served by an observed or certified multi-parent interface are
not turned into redundant weaker parent-subset rules. Different bindings, such
as a port swap or a repeated argument, remain distinct. This is proof-directed
admission from existing finite closures, not permutation enumeration, a search
budget, or an arbitrary new-target generator.

## Results after the fix

- Four shared-family cases: left only, right only, left then right, right then
  left. All complete constructor/equality states match native execution.
- Both one-sided cases have one live instance of the SAME marked Cell family;
  two-sided cases have two differently bound instances of that family.
- Before/after-union cases both match native; the guarded result stays absent
  before union.
- A separate delayed-producer example, trained with both sides present, still
  correctly handles a right-only input without introducing a left effect.
- Original inference rules do not execute on any of these seven compact paths.

This establishes these regressions, not universal soundness of every future
graft. Primitive/non-positive environments retain their existing behavior.
Additional interfaces can change solver choices and physical storage; no speed
or memory improvement is claimed.

The repeated-binding regression now also checks that the certified canonical
route can produce `Repeated(x)` for `Add(x,x)`, while a distinct-argument input
cannot. Replacing raw rows with canonical rows is no longer inherently invalid
once a matching certified boundary exists. Storage-cost checks include the
selected shared-interface columns.

## Graph repair: certified interfaces without changing history

The native registry now exports `graft_interfaces` separately from `nodes.parents`.
An independent producer-consumer window is connected to the single-application
entries for **all** of its recorded producers. The containing chronological
prefix is never substituted for those entries. Historical nodes retain separate
`producer_parents` and `context_parents` fields.

Each interface carries typed port substitutions, staged external injections,
an ordered residual replay, and validation checks. Before admission:

1. Both preparations use exactly the same positive constructor/equality rule
   environment. Functions, updates, subsumption, primitive guards, symbolic
   lowering and negative checks do not pass this new proof path.
2. Real history tokens determine the port mapping, including argument order,
   repeated arguments and sort identity. No permutation is guessed.
3. Every rebased parent preparation is an ordered subsequence of the child.
   Parent validation is also present; sibling-only effects cannot disappear.
4. Both endpoints must be in the exact completed saturation catalog. Pending,
   rejected and observation-only bodies never authorize an interface.

For the accepted fragment, with target preparation C and mapped parent
preparations P, P is contained in C. The remaining replay D supplies C when
combined with P. Monotonicity and idempotence give
`Sat(Sat(P) union D) = Sat(C)`. Checks and union obligations remain explicit;
this is not a claim that the target can be reached from just its rule name or
from any differently bound member of the parent's eclass.

The registry checks only recorded parent sets and new history windows, once per
pair, with no all-Cell pair enumeration, extra saturation or whole-graph export.
For each tested parent set it scans the already prepared programs and performs
port lookup/string comparison. Export size includes the ordered residual proof;
no speed or memory improvement is claimed. This is an additive analysis graph,
not a change to the runtime snapshot cache or emitted `.egg` rules.

`graph-results.json` records the actual native rerun. In both left/right orders,
the independent window now has a certified edge from base and contains no
opposite-side effect. The union-enabled route retains its output-alias union;
it is not represented as an unconditional Gate-only transition. All seven
compact whole-state differential cases still pass.

The website uses gray dashed edges for history and green diamonds for certified
interfaces. Clicking a diamond displays its exact binding and obligations.
The inspector can switch between interface paths and historical paths. It never
silently replaces a certificate's parent with another eclass member. Body LHS
highlights are computed for the selected route's parent set. Every round exports
its then-current interfaces; older snapshots cannot read future certificates.

```sh
# Backend graph regressions, including union enablement and snapshot admission.
cargo test --offline --release --test graft_diamond --test cell_windows
node tools/egglog_debugger/test_cell_egraph.mjs
PYTHONPATH=tools/egglog_debugger python3 -m unittest tools/egglog_debugger/test_cell_egraph.py

# After the probe above, with the local debugger running and Playwright on NODE_PATH:
node tools/egglog_debugger/test_graft_interfaces_browser.cjs out/my-graft-diamond/left-right
```

Limits: this does not discover every possible graft. With history windows
disabled, the independently re-read target may still be absent. Unsupported
programs retain their history and native replay results but get no new interface
certificate. The earlier `certified_boundary_cut` compiler path remains a
separate executable optimization with its own containment proof.
