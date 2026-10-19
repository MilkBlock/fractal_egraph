# Schedule-gated sat skip: acceptance and timing

One release execution per lane on Apple M3 Max, RAYON_NUM_THREADS=1. The same
512 held-out Math inputs and nine original schedule boundaries run on all lanes.
No timing average. This is the actual shared GPU/native runtime with positive
whole-Cell certificates, not raytrace, pristine upstream, or virtual storage.

| Lane | Nine-boundary run ms | Three saturation boundaries ms | Smooth logical matches executed | Additional runtime setup ms |
|---|---:|---:|---:|---:|
| native | 11.236 | 8.693 | 13,312 | 0.000 |
| gpu_full | 36.997 | 22.173 | 13,312 | 38.448 |
| gpu_sat_skip | 118.238 | 81.692 | 0 | 39.834 |

Bake: 3.824 ms. Shared input/schema/emitter preparation:
24.583 ms, outside the run timers. Native runs
on a clone of the same prepared engine (private emitter rules remain unused).
GPU setup includes worker/program installation; caches were not flushed.
Full-graph comparisons and reference snapshots are outside measured boundaries.
Independent native saturation/frontier audit is off in this timing run.

**Correct sat skip is demonstrated; a net speedup is not.** All 13,312 original
smooth matches disappear on the assisted path, but CPU Cell maintenance is more
expensive than the saved work in this small Math family. GPU dispatches fall
from 14 to 6. All three saturation boundaries are certified; there are no
unknown-context fallbacks in this deliberately fully covered benchmark.
Ordinary coarse/observer queries still execute. Skipped matches are not a count
of eliminated committed mutations, nor a saved-time estimate.

The 3,072 transition occurrences are submitted in **4 native batches**. An earlier
unbatched implementation took 883.426 ms with the same input/schedule and full
boundary equality. It is retained as a development comparison, not an isolated
one-change experiment or an additional sample to average into this result.

| Cell maintenance component (inside saturation hook) | ms |
|---|---:|
| Validate current graft bindings | 4.181 |
| Native batched macro actions and commit | 5.085 |
| Consume receipts/commits, maintain ownership and successor indexes | 49.572 |
| Prove component coverage | 20.575 |

Publication and coverage dominate this implementation. These timers are parts
of the hook, not extra costs to add on top of total run time. Coarse-event
observation and GPU row forwarding also cost time and remain in runtime counters.
There is no claim that the same ratios apply to expensive eggcc joins.

## Checks

- Every ordinary and saturated boundary compares all original rows, literals,
  equality and visibility against the same native execution: **18 graph checks**
  in the timing run, plus 18 in a separate 32-input check run.
- Final graph: 13,824 public rows, including Published/Start observer/input rows;
  8,192 mapped equality classes. All lanes agree, not just their counts.
- Separate check mode additionally verifies each completed shortcut by native
  saturation of a clone and audits every ordinary GPU incremental frontier.
- 31 whole-Cell/schedule tests and 31 shared-backend regressions pass. Cases
  include no early R before its scheduled saturation, unknown entries, handback
  after deletion, stale queued grafts, zero-effect registration/stop flags, until,
  changed rules, foreign merges, late union, slot recycling and delayed consumers.
- Public query cursors advance only after a complete ruleset certificate. No
  cursor is advanced merely because one component has a reusable Cell.

## Remaining scope

The current certificate is for finite, positive constructor/union closures in a
fixed verified rule environment. Destructive visibility changes disable this
observer's proof; other contracts use the original backend. Raytrace `terms`
contains numeric guards, function updates and custom merge behavior, so it is
**not yet enabled for whole-Cell sat skip**. The earlier 39-second saturation
measurement identifies potential work, not an established acceleration amount.
