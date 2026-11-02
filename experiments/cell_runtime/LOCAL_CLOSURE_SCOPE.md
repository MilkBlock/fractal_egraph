# Local dependency scope for RipSat certification

This changes native Analyze's certification scope and exports validity conditions.
It does not change which rules execute during RipSat, weaken a relevant merge or
primitive contract, or enable live Cell substitution in the CPU/GPU engine.

## Dependency calculation

1. Start with every populated declared table, including subsumed rows.
2. A rule may participate when all its relational premises may be present. Keep
   all such rules regardless of whether the observed bindings or numeric guards
   happened to match. Include their RHS constructors, reads and updates.
3. Include nested merge callback reads and constructions transitively. Repeat to
   a fixed point over the finite table/rule inventory; no search budget or
   representative sampling is used.
4. Check constructor fields, table signatures, merge semantics and primitives
   only in that slice. Include types referenced by containers and named ports.
5. For excluded rules, retain the absent prerequisite tables and the rule
   definitions. These absence conditions are part of the closure contract.

Unused `ExprSetPrim`/`ES` declarations therefore do not reject an ordinary pure
Cell. A rule which could generate `ES` still brings the container into scope,
even before that rule has actually executed. A false numeric guard is not used
as a proof that a rule is irrelevant.

No populated fact is discarded by this slicing: all populated tables are roots.
The exported `scope.dependency_contract` contains relevant tables, types,
possibly active rules, absent prerequisite tables and the invalidation policy.
Serialized recipe completion scopes retain the same contract.

## Validity and integration boundary

`closure_contract::LocalDependencies` is a typed version of the dependency list.
`Registry::watch_local(instance, dependencies, tokens)` subscribes an instance;
`Registry::apply(Change)` invalidates it on:

- insertion, function update, delete or subsume in a relevant table;
- a change in a previously absent prerequisite table;
- union or change involving an instance equality identity, including internal
  and non-root identities; union aliases are followed by the registry;
- a scope reset for changed rules, merge definitions, native primitives or
  schedule permissions.

The caller must provide **all concrete instance equality identities**, not only
the public inputs. New bindings/parameter values require a new instance check.
The monitor is deliberately table-granular: unrelated updates in a watched
table can conservatively invalidate an instance. It is not an exact per-key
minimal dependency solution.

These exported conditions and the tested validity API do not automatically
authorize the live CPU engine to substitute a terminal graph. The current
`live_substitution_authorized=false` boundary is retained. Immutable Bake
snapshots remain valid historical evidence when a runtime instance becomes
stale; it is the reuse permission for that changed instance that expires.

Positive weak-dominate/reentry queries do not require two closure dependency
slices to be identical. They still verify the same rule environment, native
runtime, relevant function definitions and actual native column types. This
does not relax completed-state equivalence. Prepared sessions share their
immutable query-environment description, avoiding reparsing/recompiling it for
each weak check.

## Original sum-bits Bake input

The same 315,811-byte input used by the structural-key repair was rerun. This
is the small compiler training input, not a full Pass 3 performance benchmark.
The 50 ms / 200,000-fact observation limits remain unchanged.

| Checked per Cell | Before | After |
|---|---:|---:|
| Tables | 381 | 7–20 (median 10) |
| Rules | 195 | 6–11 (median 7) |
| Types | whole declared environment | 5–10 |

There are still 89 discovered combinations: 72 saturate and 17 fail replay
checks. All 72 completed graphs retain exactly the same values, rows, equality
structure, subsumption mask and fixed ports as the previous exports.

The irrelevant container rejection disappears. The real remaining blocker is
now named explicitly for all 72:

```
local dependency ExtractedExpr: sharing supports only min/max lattice merges;
other merges remain uncertified
```

Thus **zero new shareable closures** are claimed for this input. The pure-cell
regression with unused container/ordered-merge declarations does obtain a
certificate and compares equivalent to its version without those declarations.

Each real Cell also retains 127–137 absent prerequisite tables (median 135).
These are invalidation conditions, not rules whose unsupported operations are
certified. This conservative frontier is intentionally reported rather than
presenting the 7–20 checked tables as the full monitoring footprint.

Per-cell counts and fingerprints: [results](local-scope-results/sum-bits-bake.json).

## Verification

114 tests pass, with four explicitly manual tests ignored. Tests cover future
rule reachability, merge callbacks, late premises, numeric guards, internal
union aliases, visibility/scope invalidation, foreign rule/merge/type rejection,
DABCABC/branching growth, weak-dominate, history, source roundtrip and Cell
equivalence. One stale `core_flow` assertion expected a single closure for both
`{A,B,C}` and `{B,C}`; the unmodified native binary also distinguishes these.
The corrected test now checks both exact row inventories and continued sharing
among identical instances.

```sh
CARGO_TARGET_DIR=/tmp/eggfuse-ripen-build CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo test --offline --lib --test ripen_local_scope --test closure_contracts --test multiple_datatypes --test reentry_history_regression --test native_set_witness --test native_structural_key_merge --test wildcard_roundtrip --test cell_egraph_normalization --test growth_reentry --test core_flow
EGG_LAYOUT_GRAPHS=0 /tmp/eggfuse-ripen-build/debug/egg_layout analyze --source /Users/mineralsteins/.codex/worktrees/d569/egg_layout/out/shared-keys-sum-bits-cpu/bake-source.egg --output out/ripen-local-scope-recheck
```

Use a new output directory. Generated graph libraries and build artifacts are
not tracked. No raytrace Pass 3 run was performed.
