# Committed `set` before-state witnesses

The trace now distinguishes unknown old state, a missing row, and an existing
row. Both dense and virtual table commit paths capture the pre-merge row;
`proposed` and `actual` retain their existing meanings. Capturing is disabled
for effects-only tracing. An unchanged merge still has a before-state witness.

Native history stores typed `SetWitness` records with stable action paths and
commit IDs. Ripen no longer rejects an entire program merely because it contains
`set` on a merged function. Instead it:

1. Restores external function rows using their recorded return values.
2. Seeds an external old row only when it has not been supplied by the selected
   stages. Checks absence before seeding, so seeding cannot silently merge with
   an unexpected existing row.
3. Checks the before-state, executes the original action/merge, and checks the
   committed after-state at that action's stage.
4. Keeps scalar output snapshots independent of later updates to the same key.
5. Rejects missing evidence, unsupported symbolic values, and incompatible
   commit ordering. Old histories default to missing evidence, not absence.

Generated entry files encode stage-local checks as `(fail (fail (check ...)))`;
absence uses `(fail (check ...))`. These retain their order even when `ripen`
ordinarily defers top-level `check` until after saturation. Native validation
still batches the original LHS/output-alias checks by stage.

This patch **does not certify arbitrary merge algebra** and does not yet recover
all update-version producer dependencies. Updated applications remain
conservatively coarse. Commit order is conservatively required to agree with
selected match/action order; interleaved histories need finer-grained replay.
Non-scalar old/results without a known symbolic term are rejected rather than
invented. Full container-valued saturated-state export is still unsupported.

## Verification

Focused tests: `native_set_witness`, plus library tests
`set_replay_preserves_absence_old_values_and_stage_order` and
`two_sets_in_one_apply_keep_each_committed_scalar_snapshot`.
They cover absent/present/unchanged rows, min merge, sequential updates,
non-idempotent addition merge (finite prefix replay only), tampered after-values,
old histories without witnesses, live/history parity, and successful automatic
saturation of a finite min-merge example alongside an ordinary rewrite.

Real eggcc run (release, no graphics), using the exporter in
`../multiple_datatypes/run.py`:

```sh
cargo build --release --offline --bin egg_layout
python3 experiments/multiple_datatypes/run.py \
  --eggcc /Users/mineralsteins/Repos/egg_related/check_statewalk \
  --analyzer target/release/egg_layout \
  --output /tmp/eggcc-set-check
```

`results.json` summarizes the completed full run and replay:
616 set receipts (313 absent, 303 present), 620 prepared composition jobs
(110 with set), 550 unique validated compositions. Still zero certified cells:
620 hit container-field export restrictions; 1,194 failed the conservative commit
ordering check; 110 lacked a representable scalar/existing symbolic value;
87 failed output aliases; 40 failed external injection; 9 had unresolved bindings.
The live/replay summaries and terminal counts agree. The ~112s live / ~78s replay
include composition preparation and are not native egglog execution timings.

Removing the former global rejection exposed a separate nested-Rayon startup
stack overflow. Session initialization is now sequential; saturation sweeps
remain parallel. Export capability errors are cached per rule environment so
unsupported sessions fail before cloning/initializing another native engine.
There is no added sampling, candidate budget, or weakened proof check.
