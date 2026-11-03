# Executable multi-stage CPU Cell recipes

The shared compiler now emits `multi-stage-program.json` alongside the typed
source recipe. `--backend cell-cpu --multi-stage-cell` uses it in the actual
standalone CPU executor. No ruleset names or terms-specific shortcuts are encoded.

The finite program contains producer-to-consumer slot projections, fixed key
reads, conditional inverse reads, primitive operations, and the original control
tree. Repeat and Saturate remain control instructions, not enumerated histories.
Symbolic column indexes find common local facts; disconnected components remain
residual queries. Rules with no fact inputs load data without generating one
specialized continuation for every literal node (Raytrace's loader has 27,238
effects). Its full program preparation took 3.26 seconds after this correction.

At runtime a committed fact or function result identifies a producer frame.
Local slots supply subsequent reads. An inverse projection is usable only when
the current visible domain is exhaustive and singleton; aliases with multiple
alternatives remain in residual matching. The entry program also handles facts
that originated in the input loader. Hash collisions can only add candidates.

Function ports retain the current producer context, **not a history per write**.
They expose the actual merge result, never the proposed value. Superseded context
references are released. Intermediate constructors, multiple proposals and union
effects are preserved by the existing typed effect executor.

Each source Run seals a `StagePatch` of bindings before executing any RHS. Effects
are published and repaired before the next Run consumes them. A finite Repeat
does not become an unbounded saturation. Late inputs, union and visibility
changes use ordinary delta notifications; an unproved region retains the exact
residual matcher. Checkpoints preserve function context ownership and continuation
statistics; only publication boundaries are resumable.

## Validation and measurements

- 31 native semantic cases, including a 28-stage chain, non-root entry,
  alias alternatives, changing function replies, delayed inputs, subsume,
  finite Repeat, Saturate and future-stage errors.
- Seven checkpoint/resume cases; source control, projection and point-program
  tampering rejected by the Rust loader independently of Python validation.
- All 31 cases also pass with forced query fingerprint collisions.
- Eight IR integrity tests; the complete SK → marker → recipe → CPU artifact
  pipeline loads and executes the three-stage function example.
- The frozen Raytrace Pass 3 has identical final state and all 11,057 Run records
  with the feature off/on. One clean run each, same binary and program:

| CPU configuration | Inference |
|---|---:|
| Multi-stage disabled | 75.178 s |
| Multi-stage enabled | 73.629 s |

This is a **2.1%** improvement, not a large speedup. It is not a pristine egglog
comparison. The existing native whole-graph discrepancies are unresolved.

There were 1,378,069 bindings completed by compiled programs, 1,793,927 executed
local projection atoms, 444 producer/consumer rule pairs and 77 distinct
cross-ruleset pairs involving 22 consumer rulesets. These are execution counts,
not distinct Cell types or committed mutation counts. The maximum executed
continuation depth was 273, including repeated ruleset visits.

Small Pass-3 off/on inference times were fact 25.14 → 23.31 ms, loopfact
26.05 → 23.10 ms, sum-bits 185.41 → 164.38 ms. The conditional ablations are
saved separately: fixed entry programs provide much of the gain; producer-local
projections alone were slower. Neither all point reads nor their savings should
be credited as closure proof reuse. On Raytrace total decoded rows were similar
(69.53M versus 69.62M), despite residual candidate visits decreasing; that decrease
alone is not a work or speedup metric.

`CELL_CPU_MULTISTAGE=0` is the full off control.
`CELL_CPU_ENTRY_PROGRAMS=0` and `CELL_CPU_LOCAL_PROJECTIONS=0` isolate the two
execution mechanisms. The existing `suite.py` handles clean control caching;
`check.py` performs the separate visibility, native and replay regressions.

This implements conditional cross-rule continuation on the CPU. Arbitrary alias
domains still require residual search, and each visible Run still performs its
publication/identity maintenance. It does not claim to replace every region by
an unconditional precomputed saturated endpoint. Numerical evidence is in
`multistage-results/`; graphs, programs and checkpoints remain untracked in `out/`.
