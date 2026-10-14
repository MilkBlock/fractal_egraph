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
| Window lineage in Cell egraph | Independent window is a root, not linked back to base |
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

## Remaining graph work

The native discovery graph is intentionally not rewritten by this patch.
Historical context parents and standalone windows remain visible as recorded.
The compiler (and merge-family evidence) can now expose additional certified
boundary interfaces without pretending they were observed mutation events.

The next graph-level repair should attach an independently validated window to
candidate base Cells only after checking its typed binding, supplied facts and
equality obligations. Keep chronological context for provenance/snapshot reuse;
do not present it as a necessary execution dependency. Union explanations may
be recorded as events or explicit boundary obligations. Never replace the
target of `CL -> CLR` with a claimed `C0 -> CLR`; the independent target is CR.
