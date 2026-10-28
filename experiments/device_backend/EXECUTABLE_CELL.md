# Analyzer-exported multi-stage Cell recipes

This page documents v1. The [v2 update extension](CELL_UPDATES.md) also handles
existing function values and multi-candidate/multi-witness contexts.

The actual GPU-owned runtime can now load an offline executable Cell artifact.
This is separate from the older `--stage-cell` file, which describes stage
contexts and scheduling but still uses generic rule matching within them.
It is opt-in (`--cell-program FILE`). No native engine change is needed.

## Discovery, proof, and execution

1. Native Analyze incrementally records observed rule applications, their
   ruleset/ordinal, a typed binding example, effect evidence and producer links
   under `cell_egraph.executable_program_evidence`. This is an evidence census,
   not a claim that every observed program is a certified saturated Cell.
2. `tools.cell_spine.executable` uses those observations to select spines, checks
   the typed rule/merge environment, and exports exact parameter slots and
   stage instructions. Source identity and typed body shape are checked; names
   assigned by Analyze are not assumed to equal the frontend's rule names.
3. A fresh process validates and lowers the artifact instructions into Metal.
   Entry admission reads the marker and its exact child ports once. Eligible
   owners bypass generic group/binding construction, including helper queries.
   A guarded original-matcher assertion rejects accidental double execution.
4. A resident per-owner instruction state and slot vector carry results forward.
   Arithmetic is checked and computed during admission. Subsequent stages
   resolve constructor handles and submit their assigned instructions. No new
   LHS search is used for these covered continuations. The certified comparison
   union seeds the local union-find; complete cost ranges still detect when an
   alias makes that shortcut insufficient.

The first admitted family is **fresh single-spine extraction**, in the three
`terms` rulesets. Inputs must have singleton cost witnesses; the target must
not already have a function result or potential candidate; the new cost must
be nonnegative and strictly below the initializer sentinel. Multiple spines,
multiple candidates, unknown ports, unsupported rules and failed guards remain
on the existing complete query path. A later alias invalidates the recipe and
resumes that path, preserving already-published evidence.

This is not a general proof generator for arbitrary `.egg` programs. The shared
helper/total-min-choice verifier was moved into `tools/cell_spine/closure_contract.py`;
only validated shapes have executable recipes. Analyzer observations select
what to generalize; observations alone are not the proof. The artifact contains
SSA-like typed operations and evidence, not a cached concrete final graph.

## Stage visibility: precompute is not publish

It is valid to precompute a recipe once its inputs are known. Its future
constructors remain symbolic instructions, and numerical intermediates remain
private slots. They do not become globally readable table rows at admission.

For a fresh extraction owner the stored continuation is:

```
terms:                   publish candidate constructors and potential
terms-helpers:           queue initial sentinel function value
comparison/block merge:  publish that stage's function result
terms-helpers:           construct comparison and queue candidate update
comparison/block merge:  merge certified equality and publish chosen result
```

The original nested schedule remains in charge of the publication boundaries.
An immutable stage snapshot alone is insufficient if another consumer also
reads prematurely changed working tables. This implementation emits the
corresponding instructions only at their stage commit. Other contexts therefore
observe the same published inputs. Union/set requests still pass through the
existing block merger, not a new CPU-side interpreter or a new host fence.
Ordinary finite `Run`s do not invoke the recipe macro.

The snapshot fast path also now distinguishes **zero matcher results** from
**zero effects**: a Cell recipe can write without producing a generic match.
Its stage changes must be recorded even when `m.matched == 0`.

A regression makes the distinction observable. `Num(1)` initially has no
extraction, while another input has an old cost 100 and a pending cost 1.
An eager whole-Cell publisher can expose `Num(1)` too soon and construct an
extra parent candidate using cost 100. The staged recipe sees the same inputs
as native and does not create that extra candidate.

## Reproduce

From the repository root (existing offline builds may be reused):

```sh
CARGO_BUILD_JOBS=8 CARGO_PROFILE_DEV_DEBUG=0 cargo build --offline --bin egg_layout
python3 experiments/device_backend/build.py --worker-only \
  --reuse-native-build out/device-build-block-cross-stage \
  --output out/device-build-cell-program-staged --target target/device-backend
python3 experiments/device_backend/test_cell_program.py \
  --build out/device-build-cell-program-staged --analyzer target/debug/egg_layout \
  --output out/executable-cell-check
```

The test calls real Analyze, builds a file using its evidence, and starts fresh
workers on held-out bindings. The manual export/load boundary is:

```sh
python3 -m tools.cell_spine.executable \
  --analysis ANALYSIS/analysis.json --core NATIVE_RUN/core.json \
  --source TRAINING.egg --output cells.json
python3 experiments/device_backend/run.py \
  --source HELD_OUT.egg --build BUILD --output NEW \
  --cell-program cells.json --stage-contexts --audit-stage-contexts --check-at-end
```

`core.json` must describe the same training rule environment. Its initial input
is not part of the portable template. The exact helper environment and typed
instructions are validated again on load. Files with corrupted hashes, forged
instructions or changed rules are rejected. `--eager-stage-merge` is incompatible
with this recipe path.

## Verification on 2026-10-28

`out/executable-cell-tests-v2/results.json`: **18 checks** including 12 complete
native/GPU graph comparisons, native evidence/export, 3 artifact rejection
checks, and 2 transactional rollback checks. All passed. Stage snapshots were
audited. Held-out results include:

| Case | Direct recipe instances | Complete rows / eclasses | Result |
|---|---:|---:|---|
| Cold nested Add tree | 4 | 29 / 18 | Exact native equality |
| Early-publication counterexample | 2 | 27 / 17 | Exact native equality |
| Multiple spines of one root | 2 leaf instances; parent fallback | 27 / 17 | Exact native equality |
| Multiple cost witnesses | 0 | 28 / 19 | Complete fallback |
| Large sentinel | 0 | 17 / 12 | Exact i64 fallback |
| Alias during closure | 1, then invalidated | 34 / 22 | Complete fallback |
| Ordinary finite Run | 0 | 13 / 11 | Schedule preserved |

The cold tree executed 36 recipe operations and bypassed 106 group visits.
These visits include repeated no-op opportunities: **they are not 106 logical
matches, mutations, or a measured time saving**. Comparison shortcuts zero
certified rows in the existing matrix batch; they do not yet compact that GEMM.
Admission and effect emission still use a serial GPU lane. No end-to-end speedup
or GPU occupancy improvement is claimed from these tiny correctness tests.

The independent existing deferred-block regression also passed all **8 checks**,
including stale block union joining, custom merge order, finite Run and rollback.
The existing comparison closure suite also passed **19 checks**, including signed
i64 extremes and cyclic multi-value classes.
The full Pass 3 native mismatch documented elsewhere remains unresolved; these
fixture results do not supersede it. Current scope is correctness of the
loadable guarded recipe and its stage visibility, not universal Cell coverage.
