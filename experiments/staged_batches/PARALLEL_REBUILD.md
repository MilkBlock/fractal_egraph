# Resident parallel block rebuild

The new ABI7 engine now uses parallel canonicalization, duplicate grouping and
stable row compaction. Actual custom merge callbacks retain physical row order.
A private scratch buffer grows on demand; match bindings and pending Cell slots
are never used as compaction scratch. Resuming after growth does not replay RHS.
All rebuild iterations stay in the GPU control loop.

An exact read-table-version test also skips quiescent seminaive Runs. Source
control, epoch and watermarks still advance. In audit mode every skipped Run
still emits its fingerprint; without audit, unchanged source control can advance
within one prepare invocation. No learned absence or candidate budget is used.

Validation:

- 19 source fixtures, repeated GPU executions; all visible rows, values,
  identities and per-Run telemetry agree with the serial rebuild variant.
- Four physical checkpoint/resume cases, including union, custom merge,
  subsume, buffer growth and saved continuation.
- Quiescence on/off agrees on all 19 fixtures, including per-Run fingerprints.
- Real Raytrace Pass 3 first 200 Runs: **43.291558 -> 3.738342 seconds**
  (checked execution; excludes compiler setup). Final 43,156 rows, 2,563 unions,
  all rows/values/visibility and canonical equality classes agree. Raw UF parent
  chains differ because parallel canonical reads do not compress paths.
  Every Run's first eight telemetry fields agrees. This pair also includes the
  lazy context change; it does not isolate that contribution.
- Continuing this same real program to Run 2,000 takes another **87.929946 s**.
  The graph has 294,967 rows. This is **not complete Pass 3**, not the requested
  2x result, and already demonstrates substantial remaining bottlenecks.

Saved real evidence (untracked binary checkpoints):
`/Volumes/Extreme SSD/eggfuse-profile/rebuild-pass3-{200,2000}`.

The historical 12.34-second full Pass 3 target remains 6.17 seconds. No prefix,
compiler-cache gain, or isolated kernel speedup counts as that achievement.
