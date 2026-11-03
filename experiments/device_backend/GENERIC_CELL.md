# Typed analyzer recipes on the GPU

This is an **exact execution path in eggfuse's experimental GPU backend**. It
uses the same generated queries, action lowering, integer arithmetic, function
merges, union-find and rebuild code as the existing backend. It does not replay
an observed terminal graph, recognize rule names, or consult a CPU matcher.

It is a correctness foundation, **not a new Pass-3 speedup claim**. Each local
instance currently executes its compiled queries serially in one GPU lane;
independent instances execute in parallel. Local indexed searches remain. This
is not a claim that arbitrary rules have become GEMM, nor that all global
matching can be skipped using a partial Cell.

## Export and execution

Normal `egg_layout analyze` now also writes `cell-device-recipes.json`:

- `programs`: shared typed tables, merge expressions, rules, operands, guards,
  complete actions, and a control program;
- `instances`: actual prepared local inputs, captured **before** ripen from its
  existing engine, with input/provenance metadata;
- `observed_completion`: unfinished instances are retained but not automatically
  executed as finite completions;
- `source_program`: the original initialized global input and original remaining
  schedule, when exportable. This is separate from isolated ripen schedules.

No second native preparation is performed by automatic export. Distinct rules
and types are not collapsed based on their names or on a few observed outputs.
Programs are shared across instances; mutable state is not. Fully equivalent
jobs that never create their own initialized session need not have a duplicate
device instance. This file is not a coverage certificate for global consumers.

Execute the already exported independent Cells:

```sh
python3 experiments/device_backend/run_generic_cells.py \
  --library out/analysis/cell-device-recipes.json \
  --build out/device-build-scheduled-reviewed --output out/device-cells
```

Execute the **original whole input and schedule**, through the existing runner:

```sh
python3 experiments/device_backend/run.py \
  --source input.egg \
  --generic-cell-library out/analysis/cell-device-recipes.json \
  --build out/device-build-scheduled-reviewed --output out/device-source \
  --check-at-end
```

That mode checks the source fingerprint, loads analyzer's typed program, and
runs it without native inference. With checking enabled, the independent native
engine executes the original source **after** device execution and compares all
facts, values, equivalences and subsumption flags. The whole-input mode is one
context, not an automatically discovered disjoint decomposition of the graph.

Old `cell-recipes.json` libraries can be converted without their training source:

```sh
cargo run --offline --example cell_recipe -- \
  out/analysis/cell-recipes.json out/device-library.json all --export-device
```

## One execution model for the four cases

1. **Properties:** predicates keep their true table keys and return identities.
   Extra reads such as a property of an operator use the same typed rule program.
2. **Correlated relations:** complete `(body, expr)` or other keys remain bound
   together. They are not independent boolean columns or a body-by-all-nodes
   Cartesian allocation.
3. **Numeric updates:** i64 guards/actions retain exact integer semantics,
   including overflow and missing reads. Function proposals use the original
   merge; custom constructor-producing merges retain intermediate evidence.
4. **Recursive growth:** new constructor rows, recursive arguments and union
   effects allocate local identities. Subsequent stages see locally rebuilt
   results and may discover additional bindings. New structure is not restricted
   to the entry's fixed carrier.

Each Run first computes all bindings from its input state, then executes actions
in the backend's declared order. Union effects are deferred through the RHS
phase and rebuilt locally before the next Run. This preserves ordinary Run
visibility while allowing a whole nested schedule to execute on the GPU.
Unchanged local indexes persist; append-only results extend them. Updates and
rebuilds invalidate the appropriate local index view.

Repeat counters, nested Saturate control, per-rule seminaive watermarks and
intermediate checks live on the device. CPU dispatches chunks of eight Runs by
default, without deciding which stage to run. The quantum is not a termination
or search budget. History buffers grow. Row/binding overflow retries only the
unfinished private instances at a larger capacity; already completed instances
are not repeated. No failed partial endpoint is returned as a successful result.

## Identity regression fixed

The previous bootstrap ordered identity strings lexicographically. Consequently
`Value(10)` could precede `Value(2)`, reversing native min-ID representative
preference. Two real terms Cells then unnecessarily rematched a helper and
temporarily created another Smaller row, despite equal final graphs.

The generic executor preserves **numeric native identity order within each
sort**. Its local IDs remain compact. This fixed both cases; all 72 retained
terms Cells now agree on each Run's matches, updated flag and row count as well
as their complete terminal states. The existing backend's default identity ABI
was not silently changed. `representative-order` is a small permanent regression.

## Verification

See `../generic_cell_execution/results.json` for recorded checks. Reproducers:

```sh
python3 experiments/device_backend/test_generic_cells.py \
  --build out/device-build-scheduled-reviewed --output out/generic-checks-new
python3 experiments/device_backend/test_generic_boundaries.py \
  --build out/device-build-scheduled-reviewed --output out/generic-boundaries-new
python3 experiments/device_backend/check_generic_analyzer.py \
  --source experiments/generic_cell_recipes/mixed.egg \
  --build out/device-build-scheduled-reviewed --output out/generic-analysis-new
```

The analyzer test enables `EGG_LAYOUT_DEVICE_ORACLE=1` to save a separate native
endpoint from the actual initialized ripen session. Those rows are **not in the
device program or its inputs**. Offline `cell_recipe --device-oracle` plus
`verify_generic_cells.py` also checks native per-Run progress of older libraries.

## Remaining boundaries

- This does not authorize substituting isolated endpoints into a live global
  graph. Overlapping instances and new cross-instance matches still need a
  global ownership/interface contract. Keeping one original-program context is
  the exact reference path, not a locality optimization.
- The generic interpreter performs local joins. Replacing them with shared
  marker/matrix continuations remains an optimization over this precise IR.
  Existing certified matrix paths and specialized terms fast paths remain.
- Only sorts/primitives/actions supported by the existing typed backend are
  lowered. Unsupported operations are explicit rejections, not dropped effects.
- Until probes and interleaved host commands are not supported by this new
  source-entry mode; ordinary execution remains available. Finite Repeat and
  nested saturation are supported without flattening them into one saturation.
- Custom schedulers, arbitrary termination, peak GPU occupancy, and a complete
  Raytrace Pass-3 acceleration ratio are not established by these tests.
