# Handwritten scheduled marker experiment

Two handwritten egglog programs executed once each on the local native engine.
No analyzer, trace collection, RipSat discovery, profiling, or rendering runs.
Cell is a manually supplied relation, not an automatically discovered RipSatCell.
131,071 keys; one P and Q per key; approximately 90% have a complete Cell tuple.
No union, later external updates, or partial Cell coverage is tested.

- baseline.egg: P(x,a), Q(a,y), External(y,z) -> Result(x,z).
- scheduled.egg: create tasks with status 0; Cell answers tasks and sets status 1;
  fallback queries only status-0 tasks; consumer reads Fragment and External.
- Both construct the same Cell relation, so its construction cost is shared.
- The implementation uses a Task/Cell Join to recognize covered tasks. It does
  not yet exploit producer bindings to emit answers directly.

Measured once, baseline then scheduled (milliseconds):

| Stage | Baseline | Scheduled |
|---|---:|---:|
| Seed generation | 26.35 | 24.19 |
| Fact production (plus tasks for scheduled) | 49.33 | 63.82 |
| Cell answer | — | 102.07 |
| Fallback | — | 13.91 |
| Consumer | 102.28 | 105.07 |
| Total including parse/setup/checks | 188.00 | 321.68 |

Scheduled is 1.71x slower. All original relation input tuples agree exactly;
both produce 131,071 Result rows. Result equality excludes engine-internal
relation output identities. A preliminary run falsely compared those identities;
the recorded run uses the corrected comparator. No averaging or best-run choice.
Snapshot comparison happens outside timing; compilation is excluded.

The schedule establishes correct fast/fallback separation in this bounded case,
but task creation, Cell lookup and Fragment materialization outweigh the saved
fallback work. This is not evidence of acceleration or general union correctness.

Reproduce from repository root:

```sh
cargo run --offline --release --example scheduled_marker_once -- experiments/scheduled_marker/baseline.egg experiments/scheduled_marker/scheduled.egg
```
