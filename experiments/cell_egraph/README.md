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
