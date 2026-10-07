# Replay newly constructed equality-valued set results

The preparation gate previously required a committed `set` output to already have
an entry in the symbolic binding map, or to be a scalar literal. A freshly created
constructor value could meet neither condition even though the original RHS
explicitly computes it. Such receipts were rejected before replay.

The new path reconstructs the RHS only when the receipt's proposed and committed
output IDs agree and the grounded RHS contains only constructors and literals.
The existing stage-local before/after checks and output-alias checks still run.
Mutable function lookups and fresh primitives are excluded from this path;
unknown merged results are not guessed or seeded as the expected answer.

Regression coverage includes an equality-valued `set`, falsified after-state
rejection, the existing scalar merge/staging tests, and a production analyze
smoke test. This changes preparation admission, not the termination criterion or
certified Cell-export policy.

```sh
cargo test --offline --release --lib native_analyze::ripen::entry::tests
cargo test --offline --release --test native_set_constructor_result
RUST_MIN_STACK=16777216 RIPSAT_SET_AUDIT_HISTORY=/tmp/eggcc-set-final/live/history.json cargo test --offline --release --lib native_analyze::ripen::entry::tests::audit_existing_equality_set_results -- --ignored --nocapture
```

The manual audit uses a test-only switch to compare the old scalar/binding-only
gate against the new gate on the same saved history and separate preparation
caches. It prepares individual recorded applications selected for equality-valued
set results; it does not run ripen saturation and is not a new full-run rejection
rate. Training histories are not tracked in Git.

## Measured result (2026-10-07)

On 101 selected applications from the saved eggcc fact history: legacy admission
accepted 3 and rejected 98; the fixed path accepted 82 and rejected 19. Thus 79
previously rejected preparations replay successfully. The remaining 19 fail
actual rule-action validation. No admission test was bypassed to count them as
accepted. The dominant full-composition output-alias failures involving TLConcat
are a separate unresolved issue. These numbers do not imply 82 certified Cells.
