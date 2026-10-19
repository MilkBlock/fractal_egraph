# Schedule-gated whole-Cell saturation

This connects `whole_cell::automatic` to the actual shared native/GPU runtime.
It does **not** add a new eager rule scheduler. The contract is:

```text
native input / ordinary Run -> commit journal -> observe/queue grafts
explicit Saturate(S)        -> validate current instances and graft facts
                              -> batch certified closure deltas through native RHS
                              -> prove coverage of every affected active component
                              -> complete: acknowledge S and omit its queries
                              -> incomplete: finish S with the ordinary backend
next scheduled Run          -> sees exactly the completed public boundary state
```

Unknown contexts are not discarded. An outer multi-ruleset saturation is not
replaced by a certificate for one constituent ruleset. Independently eligible
inner saturation boundaries can still use their own certificate.

## Correctness boundaries

`AutomaticRuntime::commit` / `step_grafts` no longer saturate implicitly. The
native `Run`, Sequence and Repeat semantics remain in effect. Only an exact
`Saturate(Run(S))` (allowing singleton Sequence wrappers) can use the current
library. `until` and changed compiled rule identities decline the shortcut.

All accepted macro effects are positive constructor/union consequences under
the certified environment. Publishing them may enable more work across Cell
boundaries. Full skipping requires the existing conservative component proof:
every affected active connected component is covered by one valid complete Cell.
Otherwise the selected normal CPU/GPU backend finishes the actual saturation.

**Watermarks are part of correctness.** Macro emission itself never advances
original query cursors. Once the **entire selected ruleset's current closure**
is certified complete, `certify_cell_saturation` acknowledges that frontier for
all its rules, including no-result cases. This is not a report of logical matches
having executed. Partial certificates do not acknowledge anything. Independent
CPU/audit query cursors inherit only this certified original frontier. A test
deletes an output afterward and checks that old unchanged inputs are not replayed.
Native `updated/can_stop` behavior is checked, including no-op boundaries.

Native table journals are draining. The Cell observer consumes its tables first,
then forwards those exact row batches, in commit order, to the GPU mirror. Other
tables use the ordinary feed; union cursors remain independent. A macro's rows
must enter that mirror before fallback or later consumers. No graph export or
fresh engine is needed per boundary. The observer temporarily borrows the paired
native engine; only check mode clones it for independent verification.

Pending proposals are rechecked against currently visible facts before effects.
Genuine deletion/subsumption (as distinct from a visible canonical replacement
after rebuild) disables this observer's positive-only proof. Foreign schema /
merge semantics also decline. These are explicit unsupported contracts, not
budgets or statistical decisions. The diagnostic report records the reason.

## What is implemented, and what is not

- Actual ordinary coarse matching can run on GPU; whole-Cell bookkeeping remains
  on CPU; all effects/union/rebuild are committed by native egglog.
- Equal transition recipes use one native action batch. Receipts preserve each
  input/target frame and are checked after rebuild. This avoids one tiny native
  invocation per Cell instance.
- Optional independent native saturation verifies complete skipped boundaries.
  Ordinary GPU fallback still has its native incremental-frontier checker.
- No virtual storage: all original public facts are materialized.
- **Raytrace `terms` is not certified yet.** Its function updates, numeric guards,
  custom merges and nested scheduling exceed this library's proof contract.
  Measuring 98% saturation time identifies a target; it does not certify those
  effects. No raytrace sat-skip speedup is claimed here.

## Reproduce the three-lane microbenchmark

One run per lane, same Math subset, same input and nine schedule boundaries:
three repetitions of `run graft 1; saturate math; run observe 1`. The final
consumer verifies that GPU queries see macro-produced rows. No full-graph
verification time, Bake, input or setup is included in the per-boundary timers;
all are separate. This is the modified native kernel, **not pristine upstream**.

```sh
mkdir -p out/cell-locality-worker
xcrun swiftc -O cell-assisted-egglog/matrix.swift -o out/cell-locality-worker/matrix
cargo build --offline --release --example sat_skip_bench --target-dir target/analyzer

RAYON_NUM_THREADS=1 target/analyzer/release/examples/sat_skip_bench \
  out/cell-locality-worker/matrix out/sat-skip-check-new 32 check
RAYON_NUM_THREADS=1 target/analyzer/release/examples/sat_skip_bench \
  out/cell-locality-worker/matrix out/sat-skip-time-new 512 time

CELL_MATRIX_WORKER="$PWD/out/cell-locality-worker/matrix" RAYON_NUM_THREADS=4 \
cargo test --offline --test whole_cell --test whole_cell_automatic --test sat_skip \
  --target-dir target/analyzer -- --test-threads=1
```

Use fresh output directories. Each run writes the complete original `.egg`,
the exact baked library, per-boundary results and runtime counters. The benchmark
compares all original facts, primitive values, equivalence and visibility after
**every** ordinary and saturated boundary; a final-count-only check is insufficient.

## Code map

- `src/whole_cell/automatic.rs`: deferred observation, validation, batched
  transition receipts, full coverage proof and scoped observer.
- `src/whole_cell.rs`: batched native emitter; existing Bake/replay proof.
- `cell-assisted-egglog/src/table_live.rs`: explicit saturation hook, fallback,
  event forwarding and synchronization with independent reference queries.
- `egglog/src/cell_assist.rs`: engine-local rule identity and trusted complete
  saturation acknowledgement. No changes to Free Join planning/execution.
- `examples/support/saturation_hook.rs`: adapter between the two crates.
- `tests/sat_skip.rs`: finite-run visibility, updated/stop flags, unknown entries,
  GPU event delivery, deletion handback, until, changed rules and foreign merges.

See [RESULTS.md](RESULTS.md) for timing, including unfavorable results. The first
unbatched implementation is retained as a storage/report artifact for comparison;
it is not averaged with the final implementation.
