# Existing Cell + boundary update

The runtime now retains resolved port wiring per live seed, including in the
joint saturation observer used by the shared CPU/GPU event loop. This is local
state continuation, not a catalog containing a separate closure for every cost.
The implementation uses native actions, merge and rebuild. No kernel changes.

`competing_updates.egg`, appended to `terms.egg`, exercises:

```text
x: cost 7          y: cost 4
    \              /
      Add candidate: 12 ─┐
                        ├─ p chooses 10 ── q chooses 15
      Mul candidate: 10 ┘

external candidate for x: cost 2
      Add candidate: 7  ─┐
                        ├─ p chooses 7  ── q chooses 12
      Mul candidate: 10 ┘
```

The two producer constructors belong to one Expr e-class. Old candidates remain
facts. Repeated improvement, a losing candidate, union of input ports, expansion
from one to several TCPair alternatives, and an unaffected independent producer
are included. The exported `program.egg` is a standalone native replay.

## What is reused

The compiled rule supplies the port order and slot assignments. The actual seed
stores immutable row handles for the resolved single-path part; rows are shared
with the current committed-row mirror. There is no saved match-history stream.

- A committed change invalidates only cached ports subscribed to that key or
  output. Deleting or replacing the seed discards its path.
- Every read address is recomputed from current bindings. A valid cached port is
  usable only if that address is unchanged. Changing x does not require another
  lookup of an independent, unchanged y.
- Missing ports are watched too. When execution stops before later ports, those
  unvisited cached ports are discarded together with their old subscriptions.
- An output with several alternatives still enumerates every branch. The shared
  linear prefix is retained; the implementation does not pick a representative
  or cache potentially mixed branch bindings.
- Row timestamps, guards, effect batches, cursor advancement and all original
  Run boundaries remain intact. Public values/equalities/visibility are checked.

Only observed seeds allocate state. Extra retained handles are bounded by the
sum of their visited fixed-program lengths. This is additional storage, not a
claimed memory compression. Genuinely branching work is not removed.

## Validation

`bench.py transition --output out/NEW` runs one small release experiment.
`bench.py report out/NEW` reuses the result. No GPU worker or Pass-3 timing run
is needed for this mode.

One execution produced the following local key/output access counts:

| Phase | Reuse off | Reuse on | Reused ports |
|---|---:|---:|---:|
| Initial competing producers | 88 | 80 | 8 |
| x 7→2, p 10→7, q 15→12 | 109 | 99 | 10 |
| Losing external candidate | 47 | 42 | 5 |
| Second improvement | 100 | 86 | 14 |
| Input union | 166 | 149 | 17 |
| TCPair becomes multi-alternative | 139 | 111 | 28 |
| Quiescent | 0 | 0 | 0 |
| Total | 649 | 567 | 82 |

These are fixed-program port-access counters, not individual hardware loads,
native Free Join probes, logical matches, or a wall-time speedup. At every
boundary the complete graph matches the same-submit native CPU reference and
the reuse-off run. The two variants submit identical per-rule binding counts.
One program compilation suffices; no concrete closure catalog is populated.
Final auxiliary state retains 56 paths and 99 shared row handles.

Evidence: `out/cell-transition-reuse-v1` and
[results/transition-summary.json](results/transition-summary.json).
The regular benchmark skill's regression suite passed, including actual shared
GPU hook tests. Dedicated regressions check unchanged operands, changed lookup
addresses, dropped subscriptions, missing inputs, aliases, subsumption,
delete/reinsert, finite Run, native handback and failure behavior.

This improvement does **not** turn arbitrary multi-producer closure into a
single macro. This fixture still has zero fresh-DAG macro instances and six
dataflow boundaries. Internal helper effect waves are retained; unchanged
ports no longer need repeated index lookup. General Analyze-to-transfer
discovery and full Pass-3 performance gains are separate claims.
