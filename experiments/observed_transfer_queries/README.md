# Observed numeric bindings → portable transfer programs

The ordinary analyzer now exports the previously rejected tuple queries and
shares the If query across numeric indices. No rule-name whitelist, manual root,
curve fitting or captured Value IDs are used.

| Case | Previous export | New export |
|---|---|---|
| If with four different `(i,j)` bindings | Four finite fixed-index entries | One complete-query program with `i,j : i64` slots |
| If trained only at `(0,1)`, used at `(8,9)` | Native fallback | Same loaded program supplies the complete binding |
| Five Get/Add-Gets/Add-All-Gets rules | Observed repetition, no executable transfer | Five checked port programs; length, offset and position are runtime values |
| Existing terms / min-plus / positive finite closures | Checked specialized proof methods | Retained; no weakening of their contracts |

**This shares execution programs, not the concrete e-classes of different
numeric closures.** The concrete RipSat catalog is not unioned across different
indices. A general query program has `complete_query` authority. It does not
authorize whole-saturation skipping, infer an arbitrary affine recurrence, or
collapse all recursive steps into one macro. The existing saturation proof
methods remain separate and can coexist in the same library.

## What changes

Analyze exports the LHS table reads actually observed for each admitted rule.
Their source positions determine anchor order. The native compiler selects the
first complete connected wiring that works; there is no candidate budget.
It retains all original atoms and guards, including alternatives not seen in
training. Rules absent from captured evidence, unsupported primitives/containers,
and disconnected queries remain in the original native ruleset.

The typed native query distinguishes variables from literals. Thus `i,j` stay
slots even if every training observation used `0,1`, whereas the literal `1` in
`pos + 1` and literal thresholds remain constants. Distinct variables may be
equal: training values do not create invented disequality or `j=i+1` constraints.

Version 3 of `transfers.egg` adds query descriptors and validated partial-key
accesses to the existing format. A cold loader verifies the shape, guards,
complete atom coverage, bound key columns and anchor. It uses the stored plan
without rediscovering a read order. Embedded original rules provide all RHS
actions, intermediate successor facts, checked arithmetic and failures.

At each original Run, loaded programs supply bindings while unselected rules
still run in the same native commit. Length changes, missing ports becoming
available, deletion/reinsertion and union are maintained through existing native
row subscriptions. No finite Run is replaced with saturation. Min-plus programs
and general queries can share a ruleset without double-running its RHS.

## Reproduce

```sh
cargo build --offline --release --target-dir target/analyzer --bin egg_layout
EGG_LAYOUT_GRAPHS=0 target/analyzer/release/egg_layout analyze \
  --source experiments/observed_transfer_queries/if-training.egg \
  --output out/if-parameter-export --bake-certificates
target/analyzer/release/egg_layout run-transfer \
  out/if-parameter-export/transfers.egg \
  experiments/observed_transfer_queries/if-new-indices.egg out/if-parameter-run
```

Use `tuple-training.egg` / `tuple-updates.egg` for the second case. Output
directories must be new. `if-same-indices.egg` verifies that different training
values produce exactly the same program wiring. `results.json` records the tiny
native demonstration; this is not a new Pass-3 performance claim.

`tests/generic_transfer.rs` checks complete original rows, values, equality and
visibility against the same-submit CPU at each Run. It covers unseen, equal and
negative indices; late/changed length ports; union; finite-wave boundaries;
unobserved native residual rules; partial-key and alias tampering; source literal
thresholds and identical checked-overflow failure. A fresh process runs after
both training source copies are deleted. The loaded program is also checked in
the shared CPU/GPU event loop; GPU cases require `CELL_MATRIX_WORKER`.
The container-rejection control uses native checks because the graph comparator
cannot encode that sort; it does not silently omit container tables from a
claimed full comparison.

## Referenced globals and two If consumers

`if-two-consumers.egg` observes both real If consumer bodies in their separate
rulesets. It exports two programs and the referenced `DUMMYCTX` initialization.
`if-global-updates.egg` supplies unseen indices, reverses the two Run stages and
then unions the global with another context. Cold execution and per-Run original
graph comparisons pass, including the shared CPU/GPU event loop.

Both surface `let` and lowered `:internal-let` plus zero-key `set` are supported.
The file retains constructor/literal definitions and required earlier global
dependencies; it does not copy native IDs, unrelated training globals or the
final value after training updates. Runtime replacements/unions still invalidate
subscribed ports. Duplicate startup definitions, mutable-table snapshots, late
initializers and injected unrelated actions are rejected. Merge `old/new` and
RHS local bindings are not mistaken for globals.

Use the same commands above with `if-two-consumers.egg` / `if-global-updates.egg`.
The saved example is `results/if-globals-transfers.egg`; its checks are recorded
in `globals-results.json`. This is query reuse across two rulesets, not a joint
saturation certificate or a new performance measurement.

The general provider currently accepts constructor-anchored table queries and
table-bound i64 comparisons. Computed LHS primitives and container columns are
not silently approximated. Local ripen's global-dependent replay/certification
is still incomplete: adding global initialization alone exposed missing read
paths and a cached-environment mismatch, and did not admit any closed state.
That trial was reverted; local closure rejection remains conservative while
portable query transfers work. Full Pass-3 automatic-library coverage and
universal recursive closure proofs remain outside these tests.
