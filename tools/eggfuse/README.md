# Unified eggfuse compilation

One entry produces and verifies:

```text
source.egg
  → source_sk.egg
  → Analyze/Bake evidence
  → source_sk_mk.egg
  → marker-program.json
  → eggfuse_related_files/{core.json,program.json,kernel.metal,manifest.json}
  → CPU reference / GPU execution
```

This is a real source transformation and execution path, not annotations around
the old full LHS. Marker consumers read the generated Unit-valued views. The GPU
validates the Marker Program against the actual typed rules and uses it for
query planning and certified completion shortcuts. The runtime executes the
emitted `program.json`; it does not silently compile a second program.

## Build and run

```sh
cargo build --offline --bin egg_layout --example lhs_ast \
  --example structural_key_source --example eggfuse_frontend
python3 experiments/device_backend/build.py \
  --output out/device-build --target out/device-target --release
python3 -m tools.eggfuse \
  --source experiments/unified_compiler/spine.egg \
  --output out/compiled-spine --build out/device-build \
  --solver-python /path/to/existing/scip-venv/bin/python
```

The solver environment must already contain `pyscipopt`; no dependency download
or solver substitution is performed. `--profile costs.json` selects with supplied
per-consumer lifecycle costs. Without a profile, the explicitly reported
**structural baseline** is used; it is not a measured performance optimum.

`--stop-after sk|analyze|markers|device|verify` exposes intermediate results.
The default reaches verification. `--backend local` routes the *same marked
program and marker contract* through the generic private-context GPU interpreter;
the default `gpu` uses the ordinary GPU query/action executor. Local whole-source
execution is a correctness/reference mode, not an automatic independent-Cell
decomposition or a promise of better occupancy.

## Pass contracts and caching

`pipeline.json` records required inputs, preserved properties, implementation and
tool fingerprints, output checksums, validation, elapsed time, and cache reuse.
Passes are structural keys, Analyze, marker selection/source rewriting, Marker
IR, device lowering, the two independent native references, and final checking.
`preserves` is a declared property, not a substitute for a verifier.

Each pass has its own output directory and receipt. A missing or edited output
invalidates that pass; failed runs never acquire a success receipt. Dynamic Bake
catalogue files are checked as a set, so adding or deleting an evidence file
also invalidates the relevant cache. Profile changes do not redo SK or Bake.
Unchanged CPU references are reused when device layout/capacity changes.

The marked source and device artifacts are available for inspection before
verification. Only a successful `verification.json` establishes acceptance;
file generation, a zero-marker plan, or a compiled shader is not a speedup.

## Three semantic layers

- **Typed Program:** native parser/typechecker output, declarations, exact
  actions/merge, and the original control tree. Unknown extension types remain
  typed `opaque` values until a target explicitly accepts/rejects them.
- **Marker Program:** typed correlated coordinates, exact constructor guarantees,
  publishers, consumers, complete-view maintenance and schedule dependencies.
  Mutable reads/guards remain explicit in consumers. A marker is not a blanket
  closed-world certificate for its eclass.
- **Device Program:** physical buffers, query plans, exact arithmetic, action
  instructions and kernels. Both current GPU executors consume the same marker
  contract. Changing this representation need not repeat native Bake.

The source-key pass performs **only source-declared choice changes** via
`; cell-choice/v1 ...`. An unannotated `to-hoist` or `:merge new` does not activate
a hidden policy. SK is the reference for marker equivalence; a declared policy
change is not incorrectly checked as equivalence to the original selection.
The emitted key extension is explicit in the `.egg` header.

## Marker formation and maintenance

The imported source adapter now offers shared constructor pairs and contiguous
constructor spines of arbitrary length. Every continuation must be fully keyed
by earlier fields. This finite, quadratic-sized source-fragment domain is **not
all connected graph subsets** and not all marker parameter layouts. All
correlated variables are retained. A solver selects non-overlapping uses.

Analyze's certified or observed constructor states select promising fragments.
Observed endpoints never authorize skipping: the exact positive view definition
and runtime completion establish the actual marker guarantee. Source tables
that may be deleted/subsumed are excluded; mutable functions stay in the
residual consumer query.

When one original RHS already supplies all evidence, a publisher is appended
after those original actions. Otherwise no publisher is invented. Complete-view
maintenance before each consumer covers initial state, late facts and new
union-created connections. Maintenance is not assumed free and has a separate
profile cost.

For a certified functional spine, a seed row has at most one full view tuple.
If a complete marker for that seed already exists in the current canonical
snapshot, GPU lowering can stop the completion query early: its sole Unit-valued
set is already a no-op. This applies **only** to the certified completion rule,
not arbitrary user RHS or an eclass's other possible matches. Missing markers
still follow exact completion. Both ordinary and local backends use this proof.

## Profile interface

The marker pass reports offered fragment keys and original consumer rule names.
A profile must match the exact SK source and cover every offered candidate:

```json
{
  "schema": "marker-cost-profile/v1",
  "source_sha256": "...",
  "provenance": "workload/stage coverage, measurement method and source data",
  "markers": {
    "fragment-key": {
      "maintenance_ns": 120,
      "saved_search_ns_by_rule": {"[\"consumers\",\"consumer-one\"]": 500}
    }
  }
}
```

Values should be consistently aggregated over the chosen workloads and stage
occurrences. Negative per-consumer savings are allowed. The optimizer subtracts
shared maintenance once and prices each selected consumer separately. Invalid,
missing or mismatched measurements are rejected; counts are not mislabeled as
nanoseconds. The included invalidation test uses explicitly **synthetic** costs,
not a performance measurement. Automatic profiler-to-candidate attribution is
not implemented by this schema.
Consumer keys are serialized `[ruleset, rule name]` pairs from the offered
fragments report, so identical names in different rulesets do not collide.

## Verification and current limits

The source comparison checks all original rows, function values, equality
partitions and visibility, projecting only the declared marker tables. GPU
checking includes the marker tables and uses an independently executed native
marked source. Input anchors must correspond; unresolved graph cycles fail as
inconclusive. Match counts need not agree after removing no-op completion work.

Regression cases cover a three-atom spine shared by two consumers, actual RHS
publication before any completion Run, incomplete publisher evidence, a late
union, distinct numeric bindings after aliasing, single-Run visibility, both GPU executors, stale contracts, cache
invalidation and explicit native structural-key choice. These are current-input
acceptance checks, not a theorem of equivalence for every future input or every
nonconfluent ruleset.

The real saved eggcc `terms` excerpt also passed the full source/GPU chain, with
214 original rows. **It selected zero markers in the present fully-keyed
constructor-spine domain.** This is compatibility evidence, not a locality win.

The native shared-DAG key extension is implemented and tested. Its nonempty
`CellStructuralKey` values **do not yet have GPU physical lowering**. They are
rejected at device compilation rather than treated as integers/hashes or silently
run under another selection policy. Key-bearing source can still be emitted and
checked natively. GPU support for that extension, wider/reverse-fiber marker
candidates, dynamic function-port caches, and full Pass-3 speedups remain work.

## Provenance

The structural-key generator, key-demand analysis, initial source-marker adapter,
native DAG extension and lossless AST printer were ported from repository commit
`74e7d9b554f34175d7fdb850f961dffd6da56b8c`
(`codex/explicit-keys-cell-integration`). No other worktree was modified or merged.
Local changes add longer source spines, observed-structure candidate evidence,
profile costing, the common contracts and target lowering. The imported source
adapter's old application-specific/two-atom limitations are not hidden.

```sh
python3 -m unittest tools.eggfuse.test_passes -v
python3 experiments/unified_compiler/check.py \
  --build out/device-build --solver-python /path/to/scip-venv/bin/python \
  --output out/compiler-contract-checks
```

## Complete Pass 3 with multi-stage Cells

`--multi-stage-cell` adds an explicit executable-recipe pass. It exports boundary
Spine recipes and candidate-update instructions from Analyze evidence, checks
those instructions against the **marked** typed source, and lowers them with
joint stages, stage contexts, block merge and matrix saturation enabled. The
manifest binds these host execution options and the recipe checksum to the
compiled kernel; loading with different options fails before GPU execution.
Missing admission or an absent eligible schedule is an error, not silent success.

For large inputs, `--bake-source TRAINING.egg` uses a smaller training program.
Its declarations/merge definitions and every nonempty training ruleset must
match the execution source's typed environment. Training may omit initialization
and use different facts/schedule. The full execution source is still used for
both native references and GPU inference. Unobserved cases retain exact fallback.
This option is explicit: it does not claim whole-input Analyze coverage.

```sh
python3 -m tools.eggfuse \
  --source /path/to/raytrace-pass-3-flat.egg \
  --bake-source out/cell-update-pass3/training.egg \
  --output out/unified-pass3-multistage --build out/device-build-scheduled-reviewed \
  --solver-python /path/to/scip-venv/bin/python \
  --capacity 1048576 --match-capacity 1048576 --multi-stage-cell
```

The currently certified joint family is `terms`, `terms-helpers`, and
`terms-helpers-helpers`, including their original nested saturation. This flag
is not certification of all Pass-3 rulesets. It does not enable Ridge prediction,
change finite Run/repeat semantics, or imply nonzero source-marker selection.
The execution receipt reports accepted scopes, stage patches, boundary/update
recipe actions and fallbacks. Recipe actions are not committed-mutation counts.
The pre-existing complete Pass-3 native/device difference must still be checked;
a completed GPU schedule alone is not a successful semantic verification.

The full Raytrace result and its known native/device validation failure are
recorded in [PASS3.md](../../experiments/unified_compiler/PASS3.md). To inspect
completed inference even when final verification failed, without rerunning:

```sh
python3 -m tools.eggfuse.report out/unified-pass3-multistage \
  --reference out/cell-spines-pass3-full
```
