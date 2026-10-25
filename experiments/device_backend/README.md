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
- i64, Unit and bool values; comparisons, checked i64 arithmetic (+, -, *, /, %), min/max, boolean integer comparisons, boolean not/and/or/xor/implication and LHS guard.
- Constructor insertion, constructor core `Set`, union and congruence propagation.
- Function `Set` with assert-equal, i64 min/max, new-value replacement and typed custom merge expressions.
- Restricted `merge old`: ambiguous concurrent first writes or conflicting rebuild collisions fail explicitly.
- Constructor subsume, including creation of an absent hidden row.
- Run, Repeat, Sequence, Saturate and combined rulesets.

Native relations have private equality return sorts; they are not treated as Unit functions. Duplicate subsumed/visible rows combine visibility using native OR semantics. Native core rules can express constructor unions through `Set`; that case is handled explicitly.

Unsupported constructs are rejected rather than silently delegated: non-Set containers, floating arithmetic, unsupported primitives/merge function reads, external custom-function RHS reads, delete, custom-view subsume, until conditions, and runtime schema/fact changes after the first Run. Bootstrap commands are restricted; includes must first be flattened. Failed checked arithmetic rejects a LHS match; in RHS it raises an execution error, matching the native distinction. Panic actions raise device error 8.

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

The saved `pass3-inventory.json` identifies 260 tables and 664 installed rules. No floating arithmetic primitive occurs in this input. The remaining export failures are 4 rules using ExprSetPrim (panic actions are now exported); further GPU compiler limitations include custom merge expressions. Export success is not execution coverage. These counts include installed rules, not just a selected schedule's executed rules.

### Integer and panic follow-up

All integer primitive names observed in the typed Pass 3 inventory now have GPU lowering: checked +, -, *, /, %, min/max and bool-< (plus the other boolean integer comparisons). Query overflow/division failure filters the current candidate. RHS failure raises error 5; panic raises error 8. Native behavior was checked directly: RHS primitive failure is an error, not a silent action-lane skip.

The expanded 28-check suite passes. This does not mean all Pass 3 rules execute: ExprSetPrim storage/rebuild, custom merge evaluation and efficient matching remain required. The reference scan/rebuild kernels must be replaced before a meaningful full-size timing run.

### Typed merge execution

Native resolved merge expressions are exported as typed trees, not reparsed from their printed strings. The GPU compiler lowers constructor applications, integer/boolean operations, and old/new parameters. Constructor creation inside merge uses an independent insertion helper, so Metal has no recursive device calls. Both ordinary Set collisions and key collisions during rebuild execute the generated merge. Equal values bypass merge, matching native behavior.

`merge new` now selects the incoming value in device commit order. This does not promise the same winner as native for concurrent competing writes; extraction quality still requires verification on the full workload. Strict ambiguity checks for `merge old` remain unchanged.

The 29-case suite passed after these changes, plus a separate native/GPU check for a union-induced key collision creating a Combine node. That latter case is now part of the permanent suite (`merge-rebuild.egg`). No full Pass 3 performance result is implied.

### Set transport and full typed export

The native exporter now recognizes Set by its container value type, exports its element sort and members, and includes equality identities inside sets in frozen anchors. Native checkpoints serialize canonical set members after rebuild. Other container kinds and nested/non-equality sets remain unsupported. Run `python3 experiments/device_backend/test_export.py --build BUILD` to test a two-element set collapsing after union.

The real Pass 3 source now completes typed export (see `pass3-export.json`), including its scheduled rules and typed merges. GPU set storage is now implemented as described below. Export coverage is not execution coverage. The existing GPU regression suite passes 30 checks; the set transport test is separately native-only.

### GPU set arena

Equality-element sets live in a variable-length arena in GPU-private storage, separate from the row section of the same allocation. Records retain a sort tag, allocation length, canonical members and normalization epoch. Set-empty/insert/union/intersect/length execute on-device. Rebuild canonicalizes member IDs, sorts/deduplicates them, interns equal sets, repairs row handles, and timestamps rows even when a set changes contents without changing its handle. Normal Run still transfers no set contents.

Diagnostic checkpoints download the arena and compare sets by typed members under the same equality bijection as constructor rows. Arena exhaustion returns error 9, never an incomplete set. The current allocation is `16 * row_capacity` words; no garbage collection or nested/base-value sets yet. This is a correctness representation, not the final indexed/parallel layout.

The 31-case native/Metal suite passes, including a set shrinking after union, recomputing its length and then continuing saturation. All 548 distinct rules in the exported Pass 3 schedule now lower to generated Metal source. This is source-generation coverage; it is not yet a completed Pass 3 run or speedup.

### Full-input layout diagnosis

Full Pass 3 generated Metal has not yet completed a successful startup/run. A monolithic entry remained in Metal compilation for several minutes and was deliberately stopped. Rulesets now have separate entry kernels containing only their own queries/actions; native schedule order and shared rule watermarks are unchanged.

The real initialization rule also exposed a buffer-layout bug: RHS temporaries were included in the global match width. At match capacity 65,536 this would reserve 13.30 GiB. The queue now stores only LHS slots (40 words including rule ID), requiring 20 MiB for that same capacity. RHS temporaries are scoped to each action case. These are computed allocation sizes, not measured RSS or speedups. The remaining huge initialization RHS should be lowered to a compact device action program rather than thousands of unrolled Metal statements.

### Device action programs and complete-key writes

RHS operations are now fixed-width device instructions. The CPU uploads them once; a GPU interpreter executes slot loads, constructors, Set, union, subsume, primitives and panic. One device register file holds RHS temporaries for the active binding. No RHS values are sent back to CPU. This prevents the initialization rule's 27,238 temporary registers from becoming a massive unrolled Metal function or a per-match allocation.

Writes use a GPU-resident open-addressed full-key index. Union invalidates this index; the next lookup canonicalizes keys and reconstructs it. No-union steps skip congruence rebuild because all writes already use canonical keys. Union-containing steps still use the reference rebuild implementation and remain an optimization target.

`--max-runs N` is a diagnostic prefix mode. Reports explicitly carry `complete_schedule: false` when execution stops before the full schedule; it must never be reported as a complete Pass 3 result. The default remains the entire source schedule.

`action-results.json` records the full 548-rule Metal compilation/startup (17.23 seconds on its first observed build), 31 regressions plus a separate prefix-control test, and three real Pass 3 Run boundaries. Initialization produced 13,457 rows / 13,456 mapped equality classes; the following two type-helpers Runs also passed complete snapshot comparison. The initialization boundary including native oracle and snapshot comparison went from 70.19 seconds with linear writes to 5.07 seconds with indexed writes. These are diagnostic prefix times, not full inference acceleration. CPU extraction and complete Pass 3 validation remain outstanding.

### Snapshot query indexes and grouped rebuild

GPU queries now use per-table chains for unbound atoms and hashed column chains when a value is already bound. A chain entry identifies both the row and column: hash collisions across tables/columns are filtered explicitly, with all original argument and primitive conditions still checked. Indexes are built on the frozen Run snapshot, not on partially committed RHS effects. Descending insertion preserves ascending physical-row traversal within a chain.

The compiler evaluates grounded filters first and chooses connected table atoms before unbound ones. For example, it can read HasType for an already bound Expr before enumerating unrelated TupleT rows. This is a complete query-plan change, not Cell-based pruning or a learned approximation.

Rebuild now groups canonical full keys with a hash index before running merge effects. Group discovery is frozen for a round; unions and newly created rows trigger another canonicalization round. This replaces pairwise whole-table comparison. Query scratch is reused for grouping after all matches have been collected.

Public entry kernels are still serial. Shared index, action and rebuild helpers are explicitly not inlined to control full-program Metal compilation. Parallel query execution and the existing matrix/Cell matching paths remain to be integrated.

`query-results.json` records 32 regressions and 256 real Run boundaries with eight complete native/GPU snapshot comparisons. The prefix ends with 44,035 rows, 37,323 mapped equality classes and 2,727 committed unions. It only reaches initialization/type-helpers/type-analysis, not the whole Pass 3 schedule. Its 123.11 seconds includes native reference execution and Python snapshot comparison, so it is not an inference-only benchmark or acceleration claim.
