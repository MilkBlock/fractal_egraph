# Proven-redundant RHS experiment

The actual native ActionBuffer now has an opt-in trusted pre-RHS filter. It runs
**after Join and complete keyed LHS reads, before enqueuing RHS instructions**.
Both in-place and scoped/batched paths are covered. Skips have their own
`SkippedRedundant` outcome and IDs; they are not successful applications or ordinary
filtered action lanes. Rollback clears the installed predicate.

The adaptive runtime supplies a snapshot of previously completed bodies. All read
facts must be contained in one body with no binding alias or current rekey taint.
Unknown/incomplete evidence or a cross-body match falls back to native execution.
For the supported positive constructor/equality environment, an injective image
of a saturated body is closed for a match whose complete premise is in that image.
This is the justification, not an empirical no-op percentage. Private instance
notifications may be omitted; no public Math consequence is omitted.

The implementation is **diagnostic, opt-in, and not a production-speed planner**.
It still performs the Join, full dependency instrumentation, owner-index work and
RHS-proof checks. It does not use a heuristic to guess that a match is redundant.

## Correctness

Separate verification processes compare normal, audit-only and audit+skip runs
on the fixed round-2 library and an eight-round math source. All final graphs are
exactly equal: **15,493 Math e-nodes, 5,150 e-classes**. The verifier establishes a
bijective correspondence using ground constructor keys and decoded primitives;
this is stronger than comparing counts. Verification/export is excluded from the
timing processes. The helper is shared with the earlier locality experiment.

30 relevant integration tests and 55 core-relations tests pass. New tests cover:
clean interior skipping, alias/union-enabled work, native UF counting, 2,048 skipped
noops mixed with 2,048 productive matches in one- and four-thread pools, and
predicate invalidation at scope reset. The existing boundary and storage tests
remain enabled.

## Fixed one-thread measurements

Three independent processes per mode, rotated order, `RAYON_NUM_THREADS=1`.
Counters below are medians, not estimated avoided operations.

| Metric | Audit only | Audit + RHS skip |
|---|---:|---:|
| LHS candidates | 13,217 | 13,213 |
| Skipped RHS | 0 | 5,540 |
| Math deduplicated-write events | 62,337 | 37,973 |
| Math inserted-write events | 17,216 | 17,284 |
| Math updated-write events | 547 | 553 |
| Native successful UF merges | 1,460 | 1,374 |
| CPU time (user + system) | 0.74 s | 0.65 s |
| Peak process RSS | 56.16 MiB | 50.89 MiB |
| Process wall time | 1.86 s | 2.36 s |

Deduplicated writes fell about 39%; CPU time about 12%; RSS about 9%.
**Wall time did not improve.** Host scheduling wait was substantial: wall times
were much larger than CPU times and varied widely. The untraced adaptive runtime
was still cheaper: 0.34 s CPU, 17.02 MiB RSS, 0.72 s wall. Thus this experiment
establishes correctness and reduced redundant work, not end-to-end acceleration
against ordinary execution. Actual insert/update counts can vary with batching,
rebuild and representative choice, even with identical logical graphs.

All timing datasets are retained: `pilot.json` initially showed lower elapsed time
(264 vs 234 ms driver medians), but later automatic-thread runs were heavily
variable and did not reproduce that improvement. `automatic_threads.json` and
`single_thread.json` are not replaced by the favorable pilot. Build time is excluded;
normal sampling/JSON output remains included. CPU time has the OS timer's resolution.
No claim is made that the previous 11-round memory failure is fixed.

## Union accounting correction

The old `UnionEvent` stream does not cover all implicit merges from `union_to_set`
and merge callbacks. Zero trace-visible union events is **not zero native unions**.
The new constant-time `native_union_count()` reads displaced UF roots, measuring
successful merges including congruence and internal metadata, not requested union
statements or distinct semantic equalities. Per-step differences are reported.

Consequently, the earlier locality report's recorded effect-free matches are not
by themselves a complete proof of semantic redundancy. This filter instead uses
the closure certificate, binding checks and independent exact graph verification.
The old attribution limitation is explicitly noted in that report.

## Reproduce

```sh
cargo build --release --offline --example adaptive_math --bin egg_layout
RAYON_NUM_THREADS=1 python3 tools/bench/rhs_skip.py --output out/rhs-new

# Explicit experimental mode; ordinary runs do not enable the predicate.
EGG_LAYOUT_SKIP_INTERNAL_RHS=1 cargo run --release --offline -- \
  reuse-adaptive frozen experiments/math_mini3/enclosure_locality/source.egg \
  out/rhs-manual experiments/math_mini3/adaptive_runtime/bake_lib_round2.json
```

The script uses an external 30-second/4-GiB guard. Full diagnostic tracing is needed
for this prototype and automatically enabled with the skip flag. The native kernel
hook trusts the caller's proof; it is not a generic license to suppress arbitrary
rules, non-monotone effects, or mismatched rule environments.
