# Event-driven state discovery, independently of query compilation

The default `analyze` discovery path is now the committed apply DAG. It does
not use CoarseLayer ownership, CSCS/CCSS grouping, a unique-anchor cut, or a
minimum subtree size to decide whether an observed transition exists.

- A witnessed insertion, table update, or displaced union admits a state delta.
  Surviving matches/deduplicated writes with no new effect are confirmations.
- Direct versioned producer dependencies and compatible bound-port contexts
  are recorded separately. Context links are not unique causal explanations.
- Same-collector-batch peers read the preceding frontier; explicit producer
  witnesses can still establish actual dependencies. Recorded equality changes
  rebase port identities. No arbitrary sequential order is imposed on peers.
- Parent state references and the current event form a DAG. Replay membership
  is materialized only for preparation. No subsets or arbitrary partner pairs
  are enumerated. Wide real/context hyperedges can still be expensive.
- Existing RHS column witnesses are reused as explicit ports, so inner nodes
  need not be inferred from layer ownership or named LHS bindings alone.
- Ripen validation and query-plan compilation remain separate admission steps.
  A rejected/unfinished ripen job does not delete its observed event.

`observed-transitions.json` reports events, parent/context distinctions,
binding ports, row/union effects and available Cell eclass links. It describes
supported captured witnesses, not every logical match. Coarse/smooth labels
are relative to recorded support; they do not substitute for closure proofs.

## What happened to layers?

Coarse/smooth remains useful terminology for external obligations. **Coarse
layer is no longer the default state or discovery unit.** The old layer code
is retained for visualization and explicit comparison:

- `EGG_LAYOUT_COMPOSITION_DISCOVERY=layers`: legacy discovery for ablation.
- `EGG_LAYOUT_LAYER_VIEW=1`: retain the layer view even without graph rendering.
- With `EGG_LAYOUT_GRAPHS=0`, default analysis builds no legacy layers.

The serialized `cs` and `CSUnit-*` directory names remain compatibility shells;
`cs.discovery=committed_apply_dag` and `candidate_kind=ObservedState` distinguish
the new semantics. Unit member lists are deltas, not flattened full histories.
Preparation consumes resolved applies directly, not `LayerStore.occurrences`.
Offline history replay preserves captured boundaries and equality-change timing.

## Accelerated runtime

The frozen runtime also discovers grafts from committed Cell receipts and the
actual binding values at LHS read slots, independent of `Compiled` query plans.
Those are compatible context grafts, not proof that a particular source caused
a new row mutation. Native unbaked rule matches lacking Cell receipts are counted
separately; full mutation/provenance evidence is available in ordinary analysis.

Binding alias matrices are interned by exact structure. `observed_grafts` refer
to shared `graft_binding_patterns`; parent and target slots use the shared Cell
coordinate system. Primitive literals remain in the Cell definitions. Distinct
symbols do not assert disequality. This avoids copying full JSON binding records
on every instance occurrence. `EGG_LAYOUT_OBSERVED_GRAFTS=0` is an ablation only.

Discovery does not authorize skipping an unproved query. The existing native
fallback and completion proofs remain the execution correctness boundary.

## Reproduce

```sh
cargo build --offline --release --bin egg_layout --example adaptive_math
python3 experiments/observed_transitions/bench.py out/my-observed-math3
cargo test --offline --test observed_state_discovery
```

The regression covers multi-parent grafts with zero admissible fast cuts,
no-effect confirmations, same-batch independence, explicit union evidence and
online/offline boundary equivalence. Existing DABCABC and branching-growth
regressions remain enabled.

`results.json` records one run per variant, not a statistical speedup claim.
Analysis and the frozen macro runtime perform different work and their counts
must not be compared as if they were the same execution. The runtime ablation
checks complete final fact/equivalence graphs. More observed contexts can make
ripen more expensive; discovery counts are not accelerated execution counts.

Preparation now uses the actual final member record ID, not its index within
the selected member list. Public interface roles exclude optional inserted-row
columns, so insertion versus deduplication cannot change the public arity.

Verified Math3 screen (one run per variant): legacy layer discovery registered
85 entries (79 saturated, 6 unfinished) in 1.92 s. Event discovery registered
92 mutating applies (26 with unresolved external fact obligations, 66 explained
by recorded support), including 70 cross-rule connections; 37 confirmations
created no state. It built zero legacy layers in graphless mode. Ripen completed
70 states and left 22 unfinished, taking 3.88 s overall: richer discovery costs
more work, and these are different entry sets, not an apples-to-apples speedup.

The accelerated runtime discovered 39 compatible grafts instead of 0, with 39
binding patterns in this sample (no measured pattern-count compression yet).
Compiled fast-query links remained 0. Both variants produced exactly 347 Math
enodes and 137 eclasses. State bookkeeping was 0.207/0.358 ms; process wall times
are cold/warm and should not be used to infer speedup. Six native source-rule
matches lacked Cell receipts and are explicitly reported, not counted as grafts.

Acceptance: 55 tests passed, with one existing expensive audit ignored. This
includes the legacy view/parity tests (explicitly requesting the layer view),
fresh-binding library reuse, and DABCABC/branching regression cases.
