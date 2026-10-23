# Explicit .egg programs with the checked Cell runtime

Integration candidate for `codex/standalone-structural-keys` (`5964b56`) on top
of `codex/automatic-refutation` (`39d99a9`). **Not accepted into the latter
branch: the full SK+marker lane still has an observed performance regression.**

This measures the real complete raytrace Pass-3 GPU/native-effect runtime. The
solver is not rerun: all three lanes use the same two previously selected loop
query layouts, shared prefix DAG and parameterized joint closure services.

## What is connected

- `Runtime::attach_joint` attaches certificates to the already initialized
  engine. Source setup and explicit structural-key schedules run exactly once.
- Program mode explicitly enables Cell assistance and selected boundary plans.
  `source` policy executes the supplied `.egg`; `legacy-stable` is the old
  in-runner policy for the control only. Combining explicit keys and the legacy
  policy is rejected.
- Offline layout rebind checks the full typed native query, indexed aliases,
  atoms, numeric conditions and selected wiring. Parser-generated variable
  names may change. Normal loading still requires the exact current setup.
- Source-declared key helper rules stay on CPU. Original admitted rules and
  native commit/rebuild follow the same shared GPU protocol as the control.
  This does not add new Cell discovery or accept the full selection policy's
  changed helper semantics as an old proof.

## Verification and timing

One execution per lane, same binary and unchanged Metal worker. Preparation,
GPU setup, graph export and Tiger are excluded from inference. These are a new
paired comparison; the historical 12.34 s from another runner is not reused as
its denominator.

| Lane | Inference | Relative to control |
|---|---:|---:|
| Original source + legacy key setup + Cell services | 12.735386 s | 1.0000 |
| Explicit `source_sk.egg` + same Cell services | 12.618744 s | 0.9908 |
| Explicit `source_sk_mk.egg` + same Cell services | 13.523112 s | 1.0619 |

The SK step has no observed regression in this pair. SK+marker is **6.19% slower**
and fails the requested merge gate. A separate CPU marker-maintenance variant
took **14.267057 s**; it is not adopted. Unchanged control/SK lanes were reused
for that variant, not rerun to select a favorable time.

All completed lanes pass exact original-table/value/equality/visibility checks
and the production Tiger selected-DAG comparison for 29 functions:

- Legacy -> SK: 699,068 original rows, 524,631 mapped e-classes; only the twelve
  added `CellStructuralKey-*` tables are projected out.
- SK -> SK+marker: 787,663 rows, including the structural-key values compared
  through their exact shared DAGs; only `CellSpineView0` is projected out.
- Each lane supplies 34,162 Cell queries / 2,041,944 logical bindings. These
  are not counts of newly committed mutations.
- Four shared prefixes retain 84 bindings each. All lanes handle 12 joint
  saturation boundaries and use 601 internal comparison batches. Fresh-DAG
  macro instances remain zero; this is not twelve free whole-cell replays.

The old source-marker solver selected six panic-check consumers and its view
produced zero rows on this input. The SK+marker lane adds two helper Runs and
one GPU dispatch; the observed total slowdown cannot be attributed solely to
that one dispatch. No new marker acceleration or universal result is claimed.
No averaging, timing tolerance, table omission or extraction relaxation was
used to turn the failed performance gate into a pass.

## Bugs fixed while connecting the paths

1. The program runner previously set an unused environment variable instead of
   disabling native probe counters. Timing lanes now use the real native API.
2. The native graph serializer prints custom base values through `Debug`.
   Derived `Debug` recursively unfolded the shared structural-key DAG and
   caused an extraction-export process to be killed. Key diagnostics are now
   bounded; hashing, exact equality/order and the full KeyGraph comparison are
   unchanged. A 60-level repeated-pair regression checks bounded formatting.
3. A child could fail after writing `complete:true`. Cache admission now marks
   any nonzero process exit incomplete, preserving its error and evidence.

The interrupted export runs are retained as failures. They are not eligible
timing/merge records. Current focused checks: **122 Rust tests and 67 Python
protocol tests pass**, including source Cell attachment, duplicate-key-policy
rejection, offline alpha rebind, stale layout rejection, finite runs, function
updates, union, missing ports and portable certificate loading.

## Reproduction

Use the versioned `cell-assisted-egglog-bench` skill:

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py explicit-keys-bench \
  --source INPUT.egg --sk source_sk.egg --marked source_sk_mk.egg \
  --layout selected-boundary-programs.json --worker MATRIX_WORKER --output NEW
```

The output records source/layout/binary/worker fingerprints, every command,
full graph comparisons, actual Cell service telemetry and Tiger checks.
Saved complete evidence is under `out/explicit-keys-assisted-v3` and
`out/explicit-keys-assisted-native-maintenance` in the automatic-refutation
checkout. Earlier `out/explicit-keys-assisted*` failures are kept separately.
