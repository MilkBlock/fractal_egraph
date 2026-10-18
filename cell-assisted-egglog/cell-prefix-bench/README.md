# Analyze-backed Cell projections in the shared GPU runtime

The recorded results here are the cached implementation at `588464e` (protocol
9). The current default adds [commit-driven interface states](../cell-event-bench/README.md)
and uses protocol 10; the runner retains the old behavior as `cell_cached`.

This is the **actual native event/schedule/RHS loop**, with an optional Cell
shortcut. It does not replace native facts, union, merge, rebuild or extraction.
The existing full-query GPU path remains the baseline and the fallback.

## What is reused

1. Import completed positive states from `ANALYSIS/catalog/`. Use the existing
   finite projection code to find typed query fragments in their bodies, at root
   and non-root positions. Keep literal and alias requirements. Do not discover
   new closures by parsing the source rules.
2. Accept only connected fragments where, after the first atom, every native
   table's **entire input key is already bound**. Each read therefore has at most
   one result, even if an e-class contains many e-nodes. Reverse reads that need
   enumeration remain in the ordinary query. There is no candidate-count budget.
3. Normalize the fragment's variable wiring. Consumers with the same fragment
   share one GPU certificate and binding frame, even with different guards/RHS.
4. On the GPU, resolve its keyed reads once and validate the analyzed
   alias/literal pattern. Store the projected binding and `(row slot, generation)`
   for the witnesses. Reuse while those exact committed rows remain visible and
   unchanged. Unknown shapes are not rejected as matches: they use the full query.
5. A query can enter this shortcut **in the middle**, after an external delta
   locates the Cell's anchor. It copies the certified binding, skips the internal
   index probes, and continues with the boundary conditions. Exact numerical
   guards and the original RHS are still executed.

These are **positive Cell projection certificates**, not a declaration that an
entire live Cell is forever closed. First admission still performs keyed reads;
the runtime does not yet infer all readiness directly from producer histories.
The benefit measured here is reuse of these reads across consumers and boundary
matches. It is neither GEMM/Tensor Core acceleration nor storage compression.

## Why this does not lose matches

For a certified anchor binding, each internal read's complete key determines
its only possible row. The stored visible witnesses supply precisely that
completion, so substituting their binding for those joins preserves the complete
query. The rest of the LHS is untouched. No certificate means ordinary execution.

The per-atom native timestamps are retained: an earlier changed atom still owns
the match under the original semi-naive ordering. A single maximum timestamp
would not suffice. Backtracking resumes before the substituted segment, not
inside a fictitious partially matched binding.

Union/rebuild changes arrive through the existing native committed-row journal.
A changed, deleted or subsumed witness invalidates its generation; slot recycling
cannot resurrect it. Adding another constructor to the same e-class does not
invalidate unrelated fully keyed reads, and the reverse query still enumerates
the new constructor. Numerical state is not erased. Index garbage collection
alone does not invalidate certificates because they refer to native row slots,
not transient index entries.

## Reproduce

Use a frozen analyze directory; do not include Analyze/Bake in runtime timing.
For the original isolated eggcc If rules, an existing library is
`out/cell-resident-verified/eggcc-if-sparse/analyze`. To regenerate one explicitly:

```sh
target/analyzer/debug/egg_layout analyze \
  --source experiments/cell_spine/eggcc_if_tiny/source.egg \
  --output out/if-cell-library --save-history
```

Build the current worker, then run each control once. `run.py` generates held-out
contexts: 10% enable min/max, 90% are near misses, using the original three rules.

```sh
mkdir -p out/cell-worker
xcrun swiftc -O cell-assisted-egglog/matrix.swift -o out/cell-worker/matrix
python3 cell-assisted-egglog/cell-prefix-bench/run.py \
  --analysis out/if-cell-library --worker out/cell-worker/matrix \
  --output out/cell-prefix-time
python3 cell-assisted-egglog/cell-prefix-bench/run.py --skip-build \
  --analysis out/if-cell-library --worker out/cell-worker/matrix \
  --output out/cell-prefix-check --mode check
python3 cell-assisted-egglog/cell-prefix-bench/run.py --skip-build \
  --analysis out/if-cell-library --worker out/cell-worker/matrix \
  --output out/cell-prefix-profile --mode profile
```

Controls: cached native queries with the same sorted commit, full GPU, Cell-guided
order only, and the identical order with Cell certificates enabled. Each compares
all final public rows, values, visibility and equality against the CPU control.
The checked run independently compares the complete native match frontier at
every boundary. Instrumented work counters must not be treated as clean timings.

`join_probes` counts entered indexed join stages, not row candidates or matches.
`skipped_join_probes` counts the stages copied from a certificate. Also report
`certification_probes` and certificate memory; skipping a stage can additionally
avoid descending a branch owned by an earlier semi-naive anchor, so counters are
not a simple conservation equation.

For a real captured packet, the existing `rule_repl` supports:

```text
:until /3/repeat0/4
:bench-cells measured /absolute/path/to/worker /absolute/path/to/analysis profile
```

Do not claim that this If library covers raytrace's terms/Smaller rules. A matching
analyze catalog is necessary for a Cell benefit. Arbitrary unsupported primitive
expressions remain subject to the shared runtime's compiler restrictions.

## Remaining costs

The device currently checks certificate generations after changed batches; it
does not yet have per-instance reverse wake lists. With N physical row slots,
K distinct projections, maximum L internal reads and B projected fields, the
certificate allocation is `4*N*K*(2+2*L+3*B)` bytes (capacity-rounded N).
Generation checking is O(N*K*L) per changed batch; only stale/absent certificates
perform keyed reads. This is bounded by the live row high-water mark, not the
length of match history. Unsupported and reverse joins still cost ordinary
search. These limits are explicit; no arbitrary-Cell coverage or overall eggcc
speedup follows from the small experiment.
