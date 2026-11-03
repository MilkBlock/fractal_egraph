# Scheduled GPU patches

This implements original schedule evidence and a GPU-owned, private multi-stage
matrix completion path. **It does not finish the broader plan of lowering every
RipSat Cell's effectful/growing recipe into one GPU program.** The new fast path
is currently a complete positive fixed-carrier fragment; the existing terms
specialization and ordinary GPU execution remain in place for other semantics.

## Control and representation

`src/cell_schedule.rs` executes original Run leaves in native egglog, including
until, and records the source position, enclosing loop iterations, record range,
updated and can_stop. Repeat uses can_stop; Saturate uses updated. Analyze keeps
these boundaries in saved history and exports the shared original control tree
and events in `cell-recipes.json`. Application replay boundaries are explicitly
labelled separately; they are not original Run boundaries.

`--schedule-library` loads this analyzer evidence, checks the source fingerprint
and control tree, and independently replays the reported control decisions.
Truncated or wrong-position evidence is rejected. The current protection pins
the source input as well as its rules; cross-source evidence reuse is not enabled.
The typed compiler proves the admitted execution fragment independently; trace
frequency or a cached endpoint does not prove completeness of matching.

The matrix compiler admits maximal neighboring original schedule subtrees when:

- All writes are unary equality-key constructor predicates with private result
  identities. The compiler checks identity use in other rules and merge callbacks
  as well. No consumer may read those private identities to generate new carrier
  positions.
- Each structural spine is one unchanged anchor row, or a correlated identity
  spine whose conditions and outputs use the same key.
- All remaining conditions read the shared predicate coordinates. Literal and
  alias constraints on the anchor are retained.
- There are no function updates, unions, deletions, subsumption, primitive guards
  or mutable structural anchors **inside that admitted subtree**.

Unsupported subtrees retain the original stage executor. In particular, a union
between two admitted regions is published/rebuilt before the next region obtains
its current canonical coordinates. No missing Cell or unobserved match is dropped.

GPU control bytecode retains Sequence, bounded Repeat, nested Saturate and Run.
Its stack is sized from the actual program depth, not from a three-stage limit.
Until is implemented by a fresh naive GPU control query at each attempted Run;
its result is not cached as permanently true. Such queries form publication
boundaries and are not admitted into the matrix program.

The initial GPU pass obtains candidate spine ports once. A compact reference
array allocates one entry for each actual marker/set dependency, rather than a
row times every plan reservation. Indirect dispatch selects only the current
stage's rows for gather, matrix multiplication and apply. Future predicates can
be absent at entry and become available through earlier patches.

A Run gathers exact 0/1 inputs before applying any outputs. Those gathered columns
are its immutable input view, so positive state can be updated in place without
copying the whole state at every stage. The next Run reads the updated coordinates.
The existing FP16/FP32 separator certificate and exact positive checks remain;
audit also verifies negative decisions. This is deterministic fixed computation,
not a trained ridge model.

Only the end of the admitted subtree materializes the accumulated rows into the
global tables. Initial hidden rows stay hidden. Birth epochs and per-rule watermarks
are retained, including combined-rule membership. Failed capacity/overflow checks
do not publish a partial private completion. Runtime buffers and control-history
storage grow with actual size; the eight-dispatch submission batch is not a limit
on the number of iterations. No new discovery budget was added.

## Evidence

All timings below are single runs of the actual `device_backend` runtime. Build,
verification and extraction are outside inference timing.

| Workload | Control | Joint patch | Result |
|---|---:|---:|---|
| Synthetic 8,192 instances / 28 distinct stages | 0.861263 s | 0.017657 s | 48.78x versus ordinary GPU execution |
| Full Raytrace Pass 3, existing Cell/terms options retained | 41.328840 s | 41.334202 s | effectively unchanged |

The synthetic program executes 56 original Runs (the bounded Repeat stops when
unchanged), publishes 229,376 new rows in one batch, and serves 221,184 successful
binding input reads from preceding patches. The complete 237,568-row graph agrees
with **independent execution of the original native schedule**, not just replay
of GPU-reported steps. This is a deliberately favorable fixed-carrier workload,
not a Raytrace speedup or a pristine CPU comparison. The gain is mostly bulk
publication (675.6 -> 2.62 ms) and preparation (134.8 -> 1.33 ms); matching
changes only from 5.27 -> 4.88 ms. It is not a 48.78x matching speedup.

The full Pass-3 compiler newly admits only `is-resolved`: 36 scope invocations,
782 logical Runs and 23,230 published rows. This scope was already handled by the
old single-ruleset matrix optimization, so the general controller adds little
work reduction here. Its measured GPU work is only about 0.066 s. The three terms
rulesets still use the previous specialized implementation. `always-run`, type
construction and other growing/effectful stages are not silently reclassified as
fixed-carrier programs.

Both full GPU lanes have 699,784 rows / 524,742 eclasses and identical full typed
facts, function values, equality and visibility. Production Tiger returns the
same 29 functions / 8,019 selected rows. The pre-existing difference between this
GPU commit protocol and fused native/pristine execution is not resolved here;
this full-run check is explicitly GPU-to-GPU. Small scheduling tests additionally
compare with independent original native schedule execution.

Validation:

- Existing generic recipe regression still has 98 prepared programs and 97
  verified finite endpoints; the 17 existing terms preparation failures remain.
- 74 Rust library tests pass; 4 pre-existing manual audits remain ignored.
- 15 new GPU/native cases cover finite five rounds, nested saturation, zero
  Repeat, cycles, ordinary Run visibility, external union/subsume/min merge,
  more than 256 Run records, until invalidation, reserved-looking user names,
  late producer repair, escaping-result rejection, and 28 stage types. They compare per-Run progress and full graphs.
- A separate capacity rejection leaves the published state byte-identical;
  the following ordinary Run also checks that watermarks were not advanced.
- 16 existing matrix cases pass, including finite-Run, alias, visibility,
  combined-rule watermarks, resume and capacity regressions.
- Analyzer -> saved library -> new worker passes. Three deliberately malformed
  stage histories are rejected.

The timings were captured with the worker fingerprints in `results.json`.
Subsequent review added private-name collision protection and a scratch-arity
check, plus complete adjacent-region selection through missing-producer cases; these are covered by the final regressions, not relabelled as a fresh full
performance run. No new hardware occupancy measurement is claimed. The existing
Metal profiler's ordinary-Run replay adapter cannot yet replay this new opcode 8.

## Remaining work for the full Cell plan

1. Lower arbitrary analyzer-exported **effect programs**, not merely use their
   schedule/provenance alongside a newly proved fixed-carrier fragment.
2. Allocate and wire the finite future structural slots of a baked Cell, with
   correct function-merge evidence and local equality changes.
3. Maintain those Cell instances across external changes, so reentry can refresh
   only affected ports instead of reacquiring the structural region.
4. Compose these programs across the remaining Pass-3 stages and extend the
   existing profiler adapter to measure their actual occupancy and useful work.

The synthetic result shows that jointly forwarding patches can remove substantial
search and publication work. It does not establish that enough of Pass 3 is yet
expressed in that representation.

## Reproduce

```sh
# Reuse the unchanged frontend where applicable; compile the current Swift worker.
python3 experiments/device_backend/build.py --output out/scheduled-build \
  --target target --worker-only --reuse-native-build out/device-build-scheduled-final

# This suite uses a frontend including the new until control-query lowering.
EGGFUSE_MATRIX_AUDIT=1 python3 experiments/device_backend/test_scheduled_matrix.py \
  --build out/scheduled-build --output out/scheduled-tests-new
python3 experiments/device_backend/test_matrix_sat.py \
  --build out/scheduled-build --output out/scheduled-legacy-new

CARGO_BUILD_JOBS=8 CARGO_PROFILE_DEV_DEBUG=0 cargo build --offline --bin egg_layout
python3 experiments/scheduled_cell_patches/check_library.py \
  --build out/scheduled-build --output out/scheduled-library-new
python3 experiments/scheduled_cell_patches/bench.py \
  --build out/scheduled-build --output out/scheduled-throughput-new

python3 experiments/device_backend/run.py --source INPUT.egg \
  --build out/scheduled-build --output out/scheduled-run-new \
  --schedule-library ANALYSIS/cell-recipes.json --check-at-end
```

For full Raytrace use the same source and configuration on both lanes:
`--capacity 1048576 --match-capacity 1048576 --cell-program LIBRARY-v3.json
--stage-contexts --matrix-saturate --check-every 0 --save-state`, adding
`--scheduled-matrix` only to the candidate. Use existing `compare_runs.py` and
`extract_pair.py`; do not compare node counts alone.
