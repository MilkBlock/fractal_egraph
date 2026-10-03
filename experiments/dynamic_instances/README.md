# Committed dynamic instances in the positive macro runtime

This changes the actual `reuse-adaptive frozen` execution path. Frozen mode
now defaults to the cell machine when no tracing/audit override is requested.
It does not change ordinary `analyze`, and is limited to the existing positive
Math constructor runtime. The analysis Cell-egraph catalog and the adaptive Bake
still have different schemas; this change is not an automatic bridge between them.

## Execution

1. Compile certified cuts from Bake. When no explicit transition plan is supplied,
   inspect structural cuts with a unique functional anchor. Admit only a consumer
   with one supported cut/anchor pair; retain all source cells proving that pair.
   Ambiguous or unsupported consumers keep native matching. This is not Top-K or
   frequency-based selection.
2. Native rules emit complete-cell binding notifications after their positive
   recipe actions. Staged/queued promises are not committed receipts.
3. After native commit/rebuild, receipts admit or update dynamic instances.
   Proved cut keys identify compatible source/target connections; these are
   possible valid transitions, not a claim of unique historical causation.
4. Publish outgoing query views from committed notification relations in the
   `adaptive_state_dispatch` ruleset. The next step uses those views in residual
   native joins; uncovered keys continue to use the existing native builder.
5. The runtime tracks a frontier of certified cells and graft edges. It does not
   falsely certify the union of that frontier as saturated. Tier0 owns all actual
   facts, including new boundary effects while the frontier is open.
6. Packing uses previously installed canonical bindings to skip repeated work.
   A receipt below the existing minimum-reuse threshold is NOT marked packed.
   Native union causes identity/view/packed-key rekeying before reuse.

There is no raw trace requirement. Between unions only new receipts are added
to view indexes. Union currently rebuilds the Rust identity/view indexes; native
query metadata remains maintained by egglog's own canonicalization/rebuild.

## Commands and ablation

```sh
cargo run --offline --release -- reuse-adaptive frozen SOURCE.egg OUTPUT LIBRARY.json
```

Optional `EGG_LAYOUT_CELL_TRANSITIONS=plans.json` supplies explicit certified-cut
specifications. `EGG_LAYOUT_DYNAMIC_INSTANCES=0` restores inline projections and
per-batch packing while keeping the same compiled cell machine. Set
`EGG_LAYOUT_CELL_MACHINE=0` to disable the cell machine entirely.

`report.json` includes `dynamic_instances`, `state_transition_seconds`, execution
and packing times. `dynamic-dispatch.egg` contains the post-commit dispatch rules.
`current` entries hold direct certified-cell references and graft edges; following
those edges describes longer chains without copying full histories. They are not
whole-context saturation certificates. Unknown grafts still run natively; new Bake
recipes are not automatically synthesized by this module.

## Checks

```sh
cargo test --offline --lib reuse_graph::adaptive::dynamic_instances
cargo test --offline --test reuse_cell_machine --test reuse_cell_transitions
cargo build --offline --release --example adaptive_math
python3 experiments/dynamic_instances/bench.py out/my-dynamic-instance-bench
```

The T -> U -> V test now asserts dynamic state revisions and verifies automatic
plan discovery against explicit plans. Existing union, incomplete recipe,
parameter symmetry, uncovered-key and parallel query tests remain enabled.
The packing regression distinguishes observed receipts from installed storage
and verifies union rekeying.

The benchmark compares the same source/Bake/binary, once per variant, and checks
complete final Math fact/equivalence graphs using `math_graph_equal.verify`.
It excludes training and compilation. Single-run timing is a smoke screen, not a
stable speedup claim. Current RSS is not peak RSS.

Verified screen: legacy/dynamic final graphs are exactly equal (347 Math enodes,
137 eclasses). Native execution was 1.353/1.303 ms; dynamic state work 0.137 ms;
packing 0.398/0.347 ms. Do not infer speedup from these single sub-millisecond
stage differences or the cold/warm process wall times. Math3 admitted 65
instances but no state revisions, so it does not demonstrate transition reuse.
The automatic T->U->V regression admitted 3 instances with 2 state revisions
and no unresolved bindings; final graph counts matched the frozen control.
