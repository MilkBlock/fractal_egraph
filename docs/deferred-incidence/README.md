# Measurement files

Base: `0f3a4fda` (`27f098a2` plus opt-in lifetime research). Standalone Cell CPU,
Apple M3 Max, Rust 1.94.0, ordinary release, one thread, no PGO. The timed
executables enable `deferred-incidence,view-ports,storage-metrics`; existing
origin-demand and entry-retirement modes are not enabled in these matrices.

- `unrestricted-*`: five measured rounds per mode, plus excluded round zero,
  on the original two frozen 64-unit artifacts. The executable predates only
  admission-hint support and its report count.
- `admitted-*`: five measured rounds per mode, plus excluded round zero, on
  cross-workload admitted artifacts. The final runtime independently validates
  their typed proofs. Each input's donor is the other workload's unrestricted
  retirement profile. Both choose the same typed `AddContext` signature.
- `audit-full-*` and `audit-admitted-*`: separate diagnostic binaries with
  `slot-transfer-audit,spine-shapes`. Their round zero is a verification run,
  not a warm-up or a performance sample.

Each summary records binary and input-file SHA-256 values. Each sample records
the frozen graph/schedule checks, logical and physical work, inference and CLI
time, RSS through inference, and per-phase timings. The admitted Spine checks
normalize only the program-identity header after removing the admission hint
from the source artifact. Raw and normalized Spine hashes are both retained;
the full trajectory, static entries, RHS, writes and resumptions are checked.

`round` identifies a group containing off, edges and on in rotated order. Use
rounds 1–5 for timing. Do not combine medians from the two matrices or add
independently computed phase medians. The workstation was not isolated; the
observed Raytrace gain does not establish a portable default improvement.

The supplementary sum-bits collision run used `CELL_CPU_HASH_BITS=0` and
`CELL_CPU_SIEVE_COLLISIONS=1`. Its graph and four ordered audit digests matched
the frozen reference in `../view-consumer-results.json`; it is excluded from
all performance samples.
