# Pre-Join root certificates

The actual native Free Join engine can now exclude a certified root slice **before
any Join stages**, without full tracing or constructing complete LHS matches.
This is opt-in (`EGG_LAYOUT_PRUNE_INTERNAL_JOIN=1`). It is not the old post-Join
RHS filter, and it does not hide global facts or alter hashcons.

## Proof boundary

For a source rule's positive constructor-tree LHS and a canonical root, the
runtime considers every matching constructor alternative at each pattern position.
All these premise rows must belong to the **same** observed saturated body.
Variables end the traversal. Ignoring literal and repeated-variable restrictions
makes the test conservative: extra alternatives can prevent reuse but cannot
suppress a productive boundary match. Injectively instantiated, unchanged body
bindings are required; aliasing or rekeyed bodies lose their certificates.
Rule environment identity is checked by the existing Bake loader.

The kernel identifies the unique root atom by its table and output variable: its
output is not consumed as an input key of any other atom. This handles frontend
variable renaming and repeated occurrences of the same operator. Ambiguous or
missing anchors fall back to ordinary matching. Semi-naive and constant subsets
are intersected first, then only the root output column is scanned into a retained
row subset. Descendant joins do not run for excluded roots. The same mechanism is
used by serial and parallel plans.

Certificates are scoped to a single step, cleared on normal/error return, and not
copied with the database. New rounds rebuild evidence against current facts.
External alternatives, new unions and uncertain evidence retain native execution.
The public kernel API is trusted: callers must supply the redundancy proof. It is
not a generic heuristic license to suppress arbitrary rule actions.

## Current limitation

Proof preparation still scans the current graph and indexes observed bodies each
round. It has no full event trace, effect attribution, or historical boundary
analysis, but it is **not yet an incremental certificate index**. The initial
experiment reduced matches while increasing total time. It is deliberately not
enabled by default. The initial slow result is preserved in `initial.json`.

## Reproduce

```sh
cargo test --offline --test reuse_join_pruning
cargo build --release --offline --example adaptive_math
RAYON_NUM_THREADS=1 python3 tools/bench/join_pruning.py --output out/join-new --runs 3
```

The benchmark uses one fixed round-2 library, the same eight-round Math source,
and the same binary. Both normal and pruned timing runs are untraced. All proof
preparation is included. Exact constructor/eclass correspondence is checked in
separate processes outside timing. This compares two adaptive-runtime modes,
**not** pristine egglog. The existing external 30-second/4-GiB process guard is a
measurement safeguard, not a matching heuristic.

Regression tests cover fewer completed native matches, nested same-operator
patterns, external alternatives, union-enabled work, 1/4-thread execution and
certificate expiry. Original RHS-skip and virtual-fact regressions remain enabled.

## Measured result

Three alternating independent runs per mode, one Rayon thread. The final untraced
Math8 comparison (`final.json`) is:

| Metric | Normal | Pre-Join pruning |
|---|---:|---:|
| Complete native matches | 13,213 | 10,580 |
| Driver median | 132.12 ms | 165.39 ms |
| Native step plus certificate preparation | 29.91 ms | 47.64 ms |
| Peak process RSS | 18.84 MiB | 25.20 MiB |

Exact final graphs agree: **15,493 e-nodes / 5,150 eclasses**, with bijective
constructor/equivalence verification, not just equal counts. Completed matches
fall by 19.93%, but driver time increases by 25.18%. Removing unrelated audit
bookkeeping improves the implementation over the initial experiment, but does
not establish a net speedup. The remaining cost is evidence/index preparation
and observation, not a hidden full LHS trace. Future incremental maintenance
must be measured against this untraced control, not against the slower RHS-audit
prototype. No default behavior change is justified by this result.
