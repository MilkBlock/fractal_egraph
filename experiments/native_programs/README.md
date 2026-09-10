# Native program generalization repairs

All execution results below are from the modified egglog runtime. Blocks remain
an additional diagnostic index; no e-node storage replacement or memory saving
is claimed.

## Implemented and verified

- `run_program_with_trace` invokes native schedules without stripping commands.
  Full CYK scopes/checks, Fibonacci's original seven steps, until, rollback and
  restoration after an error pass. Dependency-only cached plans preserve keyed
  LHS bindings; ordinary plans are unchanged.
- Deletion invalidates only the removed row. Opaque dependency observations use
  valid individual outputs; an invalidated side output cannot erase them.
- Union-find records commit decisions, including redundant unions. For traced
  equality paths, native rebuild records source row version, new write outcome,
  and supporting union event IDs. Missing/untraced paths remain unknown.
  Coalescence records its actual insertion/deduplication/merge outcome; an
  Updated version is not credited as an independent newly produced fact.
- Block interactions distinguish union evidence from Inserted row evidence.
  Scope rollback retires old blocks. Full recipe revalidation checks physical
  input/internal/output versions and incoming block dependencies; it rejects
  deletion followed by untraced reinsertion even if the entry still exists.
  Blocks are not merged merely because roots become canonical-equal.
- Native literals preserve their types. The composer can lift a producer into
  a proper subtree of a consumer LHS (mul-fold -> add-fold), retain guards and
  preserve intermediate RHS evaluation. Generated rules are supplemental; keep
  original rules. They are not a replacement for all bounded schedules or error
  behavior when a later guard fails before an earlier rule would have executed.
- The viewer shows rejected candidates, committed/redundant unions, rebuild
  versions, and equality interactions.

## Reproduction

```
cargo run --bin egg_trace -- experiments/native_programs/simplify.egg results/block_viewer/simplify.json
cargo run --bin egg_trace -- experiments/native_programs/union.egg results/block_viewer/union.json
cargo run --bin egg_trace -- egglog/tests/web-demo/cyk.egg results/block_viewer/cyk.json
cargo run --bin rule_prepare -- experiments/native_programs/simplify.egg results/block_viewer/prepared.json
python3 tools/block_viewer/render.py results/block_viewer/cyk.json --output results/block_viewer/cyk.html
```

Observed: simplify checks Const 26 with three blocks; union has two blocks with
an equality interaction; full CYK's three stages have 24, 36, 66 active blocks,
with all scopes' native checks passing and zero active blocks after each pop.
The preprocessor emits 28 candidates for simplify; every emitted command passes
native typechecking, and running the augmented optimizer retains Const 26.
Guard rejection, integer overflow, and preserved literal types have focused tests.

```
CARGO_INCREMENTAL=0 cargo test --manifest-path egglog/Cargo.toml -p egglog --no-default-features --test program_trace --test primitive_dependency_trace --test dependency_trace --test union_trace
CARGO_INCREMENTAL=0 cargo test --test dependency_blocks --test native_program_blocks --test coverage --test rule_prepare --bin rule_combine
```

## Remaining explicit boundaries

This does not claim a universal equality proof engine. Untraced equalities,
initial rows without an origin, arbitrary custom merge functions, container
canonicalization and virtual/external reads may still have incomplete lineage.
The equality-path search is a diagnostic BFS, not a performance-optimized index.

Generic `egg_trace` observes runtime rules as opaque and preserves native program
semantics. `rule_prepare` exports candidates separately; it does not silently
activate them or alter schedules. Guards introducing new variables, relational
multi-atom rule composition, birewrites, and preprocessing included files are
reported unsupported by the preprocessor, while the native runner still executes
those commands. Connecting all these candidates to certified dynamic activation
is not completed by this patch. The older E-only snapshot matcher is still limited
to its fixture domain; typed candidate execution uses egglog itself.

Revalidated certificates are not automatically deduplicated across overlapping
instances. The current safe policy preserves both block identities. Preserving
intermediate evaluation also preserves intermediate nodes, so these correctness
repairs alone establish neither faster execution nor lower peak memory.
