# Port-storage experiment evidence

Base implementation: `c4f48685`. All data uses the standalone single-thread
Cell CPU executor and the same frozen sum-bits/Raytrace 64-unit inputs.

- `hash-*`: the earlier extra-hash-directory prototype, retained as negative
  measurement evidence. This implementation is not kept as a runtime option.
- `direct-*`: final shared tagged-directory matrix. Round zero is excluded;
  rounds 1–5 alternate off/on in fresh processes. Summaries contain binary and
  input SHA-256 values, medians and representation counts. Samples also retain
  individual phase times and all work counters.
- `audit-*`: independently built ordered-audit executable. Every recorded graph,
  schedule, RHS, effective-write, resume and Spine hash matched the frozen
  reference in `../view-consumer-results.json`. These timings are diagnostic.
- `census-raytrace.json`: whole registered RHS constructor-frame census from
  the final timed executable, with `--frame-census --deferred-incidence on`
  and port storage disabled. The final graph matched the frozen Raytrace hash.

Only `--port-frames` changes between paired arms. Deferred incidence, source-IR
release, entry retirement and origin-demand are off in the timing matrices.
RSS is sampled before graph export; complete CLI includes export. Slab capacity
bytes omit allocator/hash metadata and are not RSS. The logical-ID directory is
shared with full Cells, so its capacity is not additional per-port storage.

Full Cells, compact records and embedded writers must be counted together when
discussing retained producers. Their sum is unchanged in both workloads. This
experiment reduces representation overhead, not logical producer count or
global publication traffic.
