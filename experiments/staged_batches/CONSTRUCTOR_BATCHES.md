# Ordered constructor batches in the resident engine

Reuse the existing typed independent-constructor recipe pass and GPU grouping
kernels. Only contiguous eligible residual bindings are batched; Cell programs,
function updates, union and other unsupported actions form ordering boundaries.
The publisher retains its cursor and deferred union requests across each batch.
Within a batch, exact keys choose the first source ordinal, so identity allocation
and physical row order are reproducible. No proposal is accepted from a score.

Workspace grows from actual bindings/effect requests, with independent scratch
from saved Cell slots. Neighbor growth cannot replay committed RHS. Numeric
failure and constructor-prediction cases fall back to original ordered actions
before batch publication. Rebuild consumes the completed block normally.

Also enabled: parallel exact no-op checks for certified residual RHS, and local
completion tickets keyed by an exact witness row revision. Unrelated union or
visibility changes do not erase these tickets. Same-epoch changes increment a
separate revision; timestamp-only checks would be insufficient. Revision overflow
turns that witness uncacheable rather than accepting a stale certificate.

Reproduce:

```sh
python3 experiments/staged_batches/check_writes.py --build out/eggfuse-clean-paths-build --output out/constructors-final-suite
python3 experiments/staged_batches/check_writes.py --build out/eggfuse-clean-paths-build --output out/constructor-batches-bench --bench
python3 experiments/staged_batches/check_certificates.py --build out/eggfuse-clean-paths-build --output out/local-ticket-cases
```

Validation includes native complete-graph checks, exact serial/batch state and
Run-history comparison, duplicate keys, mixed set/union ordering, constructor
prediction fallback, overflow with its preceding effect retained, and two arenas
where one grows after the other has completed. The 19 general fixtures, private
Repeat fixtures and physical checkpoint suite also pass.

A single 32,768-binding diagnostic creates 65,536 constructor rows:
**0.394601 -> 0.212679 seconds**. Small batches do not consistently improve;
no speedup is claimed for them. This is a resident-engine microbenchmark, not
Raytrace Pass 3.

The full real Pass 3 with these changes, local tickets and no-op checks took
**214.363695 seconds** (9,782 Runs), versus 217.083534 seconds in the preceding
uncached path. This is only a small net improvement; the 6.17-second target has
**not** been reached. See `pass3-completed.json` for provenance and counters.
