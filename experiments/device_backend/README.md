# GPU-owned egglog execution reference

This experimental backend keeps egglog's native parsing, desugaring and typechecking on the CPU, then transfers the typed core rules and initial facts once. Metal owns the evolving tables, matching, RHS actions, equality classes and rebuild. The CPU controls the schedule. It does not download bindings or apply RHS actions on behalf of the GPU.

This is a **serial GPU correctness reference**, not a Tensor Core implementation, performance result, full Pass 3 backend, or automatic replacement of `EGraph::step_rules`.

```text
CPU native frontend → typed core rules + initial state → GPU private buffers
CPU Run command     → match → queue bindings → RHS → rebuild → status
CPU optional check  ← explicit snapshot, compared with independent native run
```

## Run

On macOS with Metal and Xcode installed, from the repository root:

```sh
python3 experiments/device_backend/build.py --output out/device-backend-build --target target/device-backend
python3 experiments/device_backend/run.py \
  --source experiments/cell_spine/manual_marker/long-after.egg \
  --build out/device-backend-build --output out/device-spine-run \
  --check-at-end
```

Output directories must be new. `build.py --target PATH` can reuse an existing Cargo target cache. The build uses the local `egglog/` sources, offline.

Verification options:

- Default: compare every Run with native egglog.
- `--check-every 3`: compare every third Run boundary.
- `--check-at-end`: finish GPU inference first, then replay the schedule natively and compare once.
- `--check-every 0`: terminate the native frontend after initialization; no native execution or snapshot downloads during GPU inference.

Source `check` assertions still run on the GPU when differential verification is disabled. They are compiled through the native frontend into isolated helper relations, listed in the exported program. Their private rows and sorts are included on both sides of verification.

```sh
MTL_DEBUG_LAYER=1 python3 experiments/device_backend/test_backend.py \
  --build out/device-backend-build --output out/device-backend-tests
```

## Ownership and implementation

| File | Responsibility |
|---|---|
| `egglog/src/device_program.rs` | Read-only typed core rule/state export and native checkpoint API |
| `frontend.rs` | Native initialization, schedule export and optional independent oracle |
| `compiler.py` | Lower typed slots, primitives and actions to Metal code |
| `kernel.metal` | Device tables, matching, effects, union-find, hashcons semantics and congruence rebuild |
| `worker.swift` | Private Metal buffers and binary command protocol |
| `run.py` | CPU schedule control and optional exact snapshot comparison |

The only changes to existing native source files are the public module declaration and a read-only accessor for the per-rule seminaive flag. AST, typechecking and planner behavior are unchanged. The experimental compiler currently generates a complete scan order over core atoms; it does **not** lower Free Join's physical plan.

Each Run freezes the device rows, matches all selected rules against that snapshot, queues bindings, executes actions, and rebuilds to congruence closure. Per-rule watermarks are shared across direct and combined rulesets. `:naive` rules bypass watermark filtering. Ordinary Run does not implicitly saturate; Repeat stops at native-style quiescence.

Rows contain table ID, visibility, timestamp, stable fact ID and typed values. Equality parents and fact redirects stay on-device. Rebuild canonicalizes equality columns, coalesces keys, performs constructor unions or function merges, combines subsumption flags, then compacts rows.

A Run sends 8 command bytes and returns 36 status bytes. Initialization, shader/program loading and explicit diagnostic snapshots are additional traffic, reported separately. No intermediate binding list crosses the boundary.

## Supported semantics

- Equality datatypes, constructors, native relations and opaque String identities.
- i64, Unit and bool values; comparisons, checked i64 addition, boolean not/and/or/xor/implication and LHS guard.
- Constructor insertion, constructor core `Set`, union and congruence propagation.
- Function `Set` with assert-equal and i64 min/max merge.
- Restricted `merge old`: ambiguous concurrent first writes or conflicting rebuild collisions fail explicitly.
- Constructor subsume, including creation of an absent hidden row.
- Run, Repeat, Sequence, Saturate and combined rulesets.

Native relations have private equality return sorts; they are not treated as Unit functions. Duplicate subsumed/visible rows combine visibility using native OR semantics. Native core rules can express constructor unions through `Set`; that case is handled explicitly.

Unsupported constructs are rejected rather than silently delegated: containers, floating arithmetic, arbitrary primitives/custom merges, `merge new`, external custom-function RHS reads, delete/panic, custom-view subsume, until conditions, and runtime schema/fact changes after the first Run. Bootstrap commands are restricted; includes must first be flattened. Arithmetic overflow fails execution instead of claiming full native undefined-primitive semantics.

Storage and match queues have explicit capacities. Exceeding them returns an error and no success report. RHS errors can leave partial device state; rollback, resumption and dynamic capacity growth are not implemented.

## Cell and marker status

The existing `experiments/cell_spine/manual_marker/long-after.egg` runs unchanged: `Spine-Summary-0`, extension, `Spine-Summary-1`, export and source assertions execute entirely on the GPU after initialization. This verifies the same explicit Cell program against native execution. It does not independently prove its transformation from the uncellified source.

Automatic Analyze certificate loading, dynamic Bake/specialization, the ridge predictor and shared GEMM matching are not connected to this backend yet. The next integration point is to replace the serial matching kernel with the existing parallel matching/matrix path while preserving authoritative device state and the differential checker.

## Validation

`results.json` records 24 checks with source/build hashes. Positive cases cover updates, combined rulesets, initial union, congruence, subsume, naive rules, Repeat and the existing Cell source. Negative tests cover capacity failures, unsupported semantics, arithmetic overflow, failed source assertions and deliberately corrupted snapshots.

The Cell source passed 12 Run boundaries, ending with 52 rows and 36 mapped equality classes (including assertion/private sorts). Checked and unchecked execution produced identical GPU step traces. End-only verification likewise did not drive GPU execution.

Comparison checks typed facts, primitive values, visibility and equality partitions up to a bijection anchored in initial state. GPU snapshots are never imported into native egglog and repaired before comparison. An unresolvable unanchored cycle is reported as inconclusive/error, not equality.

The oracle was the current modified local egglog working tree, **not pristine upstream**. The manifest records its dirty paths and source digest. The immutable `egglog-baseline` tag remains unchanged. These are differential correctness results, not performance measurements; wall times include diagnostics and must not be compared with Pass 3 inference benchmarks.

### Pass 3 bring-up

The real `raytrace-interactive-20261010/pass-3-flat.egg` was submitted to the native frontend. Boolean export was the first blocker and is now supported. The next rejection is `f64`; floating values have not been silently reduced to FP32. The boolean extension passes the 25-case regression suite with Metal validation enabled. This remains incomplete Pass 3 support.

### Typed Pass 3 inventory and floating payloads

`f64` payloads now retain their 64-bit representation on-device. The export normalizes signed zero and NaN to match native OrderedFloat identity; no conversion to FP32 occurs. This supports storage, binding, equality and copying, not floating arithmetic. The fixture includes 1e16, signed zero and NaN. The expanded 26-check suite passes.

Inventory all installed native typed rules without executing them:

```sh
python3 experiments/device_backend/inventory.py --source PATH/pass-3-flat.egg \
  --build out/device-backend-build --output out/pass3-inventory.json
```

The saved `pass3-inventory.json` identifies 260 tables and 664 installed rules. No floating arithmetic primitive occurs in this input. The remaining export failures are 49 panic-bearing rules and 4 rules using ExprSetPrim; further GPU compiler limitations include integer operators and custom merge expressions. Export success is not execution coverage. These counts include installed rules, not just a selected schedule's executed rules.
