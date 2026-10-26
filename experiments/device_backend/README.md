# GPU-owned egglog backend

The CPU keeps native parsing, typechecking and schedule control. Metal owns the evolving tables, matching, RHS actions, union-find and rebuild. Normal execution returns status/timing data, never bindings for CPU-side RHS execution.

This remains an experimental device backend with parallel index construction, matching, congruence grouping and compaction; necessary RHS effects and merge callbacks are committed serially. It has completed all 10,097 raytrace Pass 3 Runs from the original input, without CPU inference, matching the logical endpoint of the earlier device continuations. Strict final graph/selected-DAG equality with native has **not** passed; production Tiger emits 29 functions on both sides, and a separate production-interpreter check agrees on raytrace's official input plus three varied inputs. That finite-input result is not an equivalence proof. There is no full Pass 3 acceleration claim. Explicit Cell/spine/marker `.egg` programs run through the same backend; automatic Analyze library loading, the ridge predictor and Tensor Core matching are not connected here.

## Run and verify

From the repository root, on macOS with Metal and Xcode:

```sh
python3 experiments/device_backend/build.py \
  --output out/device-build --target target/device-backend
python3 experiments/device_backend/run.py \
  --source experiments/cell_spine/manual_marker/long-after.egg \
  --build out/device-build --output out/device-run --check-at-end
```

Builds use the local `egglog/` offline. Reuse an existing Cargo target with `--target`; output run directories must be new.

- Default: native comparison after each Run.
- `--check-every N`: compare every N Run boundaries; execute the native reference in step order.
- `--check-at-end`: run GPU first, then replay the native schedule and compare once.
- `--check-every 0`: retire the CPU frontend before GPU Runs; no native inference or checkpoints.
- `--max-runs N`: diagnostic prefix only. `complete_schedule: false` prevents presenting it as a full run.
- `--resume-gpu DIR`: restore a canonical GPU checkpoint without claiming native equality. Use this for device-to-device ablations or unchecked execution.
- `--snapshot-every N`: save bounded GPU checkpoints during diagnosis; this adds output overhead.
- `--resume DIR`: restore the last complete native-verified checkpoint, then continue the original schedule. `--max-runs` is an absolute schedule count, including the restored prefix.
- `--verify-against DIR`: reuse a saved, checked native endpoint for the identical core program and schedule. No native Runs are executed. This is useful for controlled ablations from the same checkpoint.

Source `check` assertions always execute on GPU, including when native verification is off. The native frontend compiles them into isolated helper relations; private/helper sorts are included on both comparison sides.

```sh
MTL_DEBUG_LAYER=1 python3 experiments/device_backend/test_backend.py \
  --build out/device-build --output out/device-tests
python3 experiments/device_backend/test_export.py --build out/device-build
```

For phase logs set `EGG_DEVICE_PHASE_LOG=1`. Diagnostic ablations: `--serial-match`, `--serial-prepare`, `--serial-rebuild`, `--scan-subsume`, `--eager-unions`, `--rebuild-query-index`, `--no-delta-queries`. Eager union is a legacy counterfactual with a known semantic mismatch, not a generally valid alternative executor.

## Execution

```text
CPU: native typed core rules + initial state (once)
                       ↓
GPU: prepare snapshot indexes
     → find complete bindings
     → interpret RHS → flush unions → rebuild
                       ↓
CPU: schedule status; optional snapshot comparison
```

Each ruleset has a match entry and shares prepare/commit kernels. Read-only rulesets assign the first branching table atom to GPU lanes. Emitted bindings are sorted on-device by original rule/variant/seed/inner order before RHS execution. Rulesets with allocating set primitives on the LHS retain serial matching; set columns and set-length reads are read-only. These stages have separate Metal command timing. RHS is fixed-width bytecode in GPU-private storage, with one device register file for the current binding. The large initialization RHS is not unrolled into thousands of Metal statements.

Matching finishes before any RHS write, so the logical snapshot aliases the row buffer safely; an entire copy is unnecessary. Stable radix partition builds query indexes in parallel while retaining physical-row traversal order. Complete-key indexes use atomic minimum representatives. Query indexes persist across Runs. Append-only changes extend chains; existing value changes, visibility changes and union/rebuild invalidate them. Keys already fully bound use direct complete-key lookup.

Only column/table/timestamp chains actually traversed by the compiled queries are built. Parallel seed rows need no chain, but serial ruleset membership is included when computing this requirement. Per-table latest timestamps are maintained independently, including for tables with no indexed columns. `--all-index-columns` retains the former layout for differential testing.

Seminaive rules partition matches by the first table atom whose row timestamp exceeds the rule watermark. Earlier atoms must be old; the pivot is new; later atoms are unrestricted. These branches are disjoint, including when multiple atoms change together or a row participates in a self-join. Timestamp buckets locate pivot rows without scanning an entire table. Naive rules keep unrestricted matching. Per-rule watermarks remain shared across direct and combined rulesets.

RHS union is queued until the action phase ends, matching native union-table insertion semantics. Immediate canonicalization can suppress a required merge constructor; `deferred-union.egg` retains that regression. This is not merely an ordering performance option.

Complete-key writes use a GPU hash index. Rebuild canonicalizes rows and forms minimum-row duplicate groups in parallel. A stable parallel scan collects only duplicate rows; callbacks execute in their previous order. Parallel compaction reuses the dead match workspace. The CPU repeats rebuild rounds based on a small status block; no rows are transferred for this control decision. It preserves subsumption and fact redirects. A no-union step needs no congruence rebuild.

Equality-element sets occupy a variable-sized device arena. Empty/insert/union/intersect/length are supported. Union canonicalizes/deduplicates members and invalidates affected row identities/timestamps even if a set's handle stays unchanged. Checkpoints transfer live arena contents only.

## Source structure

| File | Role |
|---|---|
| `egglog/src/device_program.rs` | Read-only native typed export, inventory, typed snapshots and anchors |
| `frontend.rs` | Native initialization, schedule export and optional independent oracle |
| `compiler.py` | Typed slots, delta join variants, ruleset kernels and RHS bytecode |
| `query_index.metal` | Column/table/timestamp chains |
| `row_index.metal` | Complete-key lookup and insertion index |
| `actions.metal` | Device RHS interpreter |
| `sets.metal` | Device set arena and operations |
| `kernel.metal` | Phase control, merge, union and congruence rebuild |
| `worker.swift` | Private buffers, command dispatch, timing and binary protocol |
| `run.py` | CPU scheduling, checkpoint restore and exact differential checks |

Native AST, core lowering and typechecking semantics remain in egglog. This path lowers typed core atoms rather than Free Join's physical plan. In addition to the read-only export seam, commit `2431271` fixes a pre-existing local CPU index bug: concatenated column scans require sorting by `(value, RowId)`, not value alone. The `egglog-baseline` tag is unchanged.

## Checking and timing contracts

Snapshots compare every typed row, primitive value, visibility flag and equality partition under a bijection anchored in initial state. GPU state is never imported into native egglog and repaired before comparison. Unresolvable unanchored cycles are errors, not accepted equality. The oracle is the modified local native kernel, not pristine upstream; manifests preserve source/build fingerprints and dirty paths.

Restoration verifies core IR, identities, saved GPU/native equality and boundary metadata. Parent forests, live sets, rows, counters and reconstructed rule watermarks are uploaded; indexes are rebuilt. A resumed segment is not an end-to-end workload timing.

The Python/worker protocol uses 8 command bytes and 68 response bytes (status, three phase durations and index rebuild count). Initialization/program upload and diagnostic snapshots are additional. The worker additionally reads small control blocks for sort sizes, index validity and rebuild convergence. No intermediate binding list is downloaded. Reports distinguish:

- `device_gpu`: sum of device command durations, split into prepare/match/commit. Commit includes the small status blit.
- `device_control_wait`: host dispatch/wait, including GPU duration; do not add the two.
- `native_oracle`: native Runs/snapshots, including replay for resume verification.
- `checkpoint_excluding_oracle`: transfer, serialization and comparison; `comparison` is a subset.

`progress.json` provides compact progress every 32 executed Runs. The native oracle is currently a correctness build; its timings are not a release CPU benchmark.

## Limits

Supported: equality constructors/relations, i64 arithmetic and guards, bool, Unit, opaque String and bit-preserving f64 payloads, Set of equality elements, constructor subsume, typed custom merges built from supported primitives/constructors, Run/Repeat/Sequence/Saturate/combined rulesets.

Rejected: floating arithmetic, other/nested/base-value containers, arbitrary primitives or merge-time function reads, delete, custom-view subsume, `until`, custom schedules, and runtime declaration/fact commands after the first Run. Flatten includes before use. `merge old` ambiguous writes still fail; `merge new` uses device commit order and requires workload-level quality validation.

Rows, bindings, identities, union queue and set arena have explicit capacities. Exhaustion is an error, not truncation. Failed RHS execution may leave partial state; resume only from an earlier complete checked boundary. There is no automatic rollback, growth or set-arena garbage collection.

## Evidence

Historical JSON files record incremental correctness bring-up, not a pristine or full Pass 3 benchmark. The original 13.30 GiB match-buffer plan was corrected to store only LHS slots (20 MiB at 65,536 bindings); that is calculated allocation, not RSS compression.

Real raytrace prefixes have advanced through initialization, type helpers/analysis and `always-run`, with native full-graph checks. The latest comparison and its exact scope are recorded in `execution-results.json`. CPU extraction and code-generation integration, full-schedule completion, parallel matching and end-to-end acceleration remain outstanding.

## CPU extraction handoff and merge-order investigation

`export_snapshot.py` converts a saved GPU state into typed rows, loads those rows
into the original native table schemas, and exports the production serialized
e-graph format. Loading uses fresh identities and unique canonical keys, preserves
visibility, and executes no rules or rebuild. A typed round-trip check confirms
that loading did not repair or alter the GPU graph. The bridge entry is terminal:
its native rule watermarks are not suitable for resuming inference.

```sh
python3 experiments/device_backend/export_snapshot.py \
  --source SOURCE.egg --build BUILD --run RUN --output NEW_EXPORT
```

`--save-state` exports the final GPU graph even with `--check-every 0`, so CPU
extraction does not require a shadow native inference run. Debug checkpoints now
retain `verified-*` files atomically; a later failed comparison cannot overwrite
the last checked state. Resume prefers those verified files.

At Run 2304, exact graph comparison found 70 extra GPU rows (25 TermConcat,
25 TCPair, 20 Smaller). That failure remains recorded. Production Tiger extracted
all 29 functions from both faithfully loaded states; its strict selected-DAG
check failed on three additional Concat nodes. `selected_dag.py` then compared
all function roots under only Concat associativity: all 29 agreed. Exact roots
differed for main and build_bvh. This is a narrow proof of the prefix's selected
IR modulo an existing eggcc equation, not a general equivalence test or a claim
about the final workload. The source equation is
`check_statewalk/dag_in_context/src/utility/canonicalize.egg:42`; the inspected
checkout was `ae80f82a`. Its Tiger cost model assigns Concat zero cost.

The native frontend also supports `--native-full --export-to NEW_DIR` to execute
the remaining original source schedule and save an independent full reference.
`build.py --release` builds an optimized native frontend. The current local
release reference completed the full raytrace schedule in 23.11 seconds and
produced 697,922 rows. This is not the historical 12.34-second assisted baseline.
See `extraction-results.json` for provenance, partial extraction evidence and
current limitations. Full GPU completion, final quality and the speed objective
remain unverified.

The unchecked serial full attempt was deliberately stopped after its device time
already far exceeded the complete release-native reference. Its partial timings
are retained in `extraction-results.json`: prepare and commit/rebuild dominate,
not matching. This establishes a backend implementation bottleneck, not a full
GPU success. The next work is parallel index preparation and congruence rebuild.

## Parallel backend evidence

`parallel-results.json` is a same-snapshot, one-Run comparison at raytrace step
1457. The final facts, values, equality partition and visibility agree. This is a
heavy rebuild microbenchmark, not the full Pass 3 acceleration result.

The regression suite includes large simultaneous congruence merges, capacities
smaller than a threadgroup, and 1024 bindings whose serial/parallel device states
must be byte-identical. `compare_runs.py` verifies identical inputs/configuration
before reporting ratios; `--device-states` explicitly compares device states
without claiming an independent native oracle.

Subsumption now uses complete-key lookup during the deferred-union RHS phase,
where keys remain canonical and unique. `--scan-subsume` retains the expensive
scan for a direct diagnostic comparison; eager-union diagnostics also retain it.

`subsume-results.json` records a same-device-state comparison at step 4209:
scanning the table took 46.90 seconds; complete-key subsumption took 33 ms for
the entire Run. The final device checkpoints are byte-identical. This isolates
a backend lookup bug/optimization, not Cell credit or full-program acceleration.

The regression suite includes canonical GPU-only restoration, 512-way congruence
and exact ordering of 1024 parallel bindings. The final strict extraction check
and finite-input execution check are separate; see below.

## Resumable merge-index refresh

Duplicate-group representatives are the earliest live rows, just like the complete
key index. Retiring the other rows does not itself invalidate that index. A changed
union-count still invalidates it, and compaction invalidates physical locations.

When a custom merge needs a constructor lookup after unions have changed, the
merge cursor is saved on GPU. The worker runs parallel index refresh kernels and
continues from that cursor. Refresh hashes canonical keys without modifying rows;
merge callback order is unchanged. `--serial-merge-index` retains the former path.

The same-state step-6232 comparison is in `merge-index-results.json`: 7.25 seconds
became 0.14 seconds. All rows, values, visibility, sets and equivalence classes
agree; parent-pointer path compression differs. This is an isolated Run result,
not full-workload acceleration. The suite now has 43 checks, including a lookup
whose key becomes canonical during rebuild.

Reports separate startup, schedule execution, state-export I/O and validation.
`inference_seconds` is provided only without periodic checks, intermediate snapshots,
phase logs or Metal validation. The start allocation metric is Metal resource
allocation, not process RSS. A resumed suffix is never a full-run timing.

## Used-column indexes and typed seed dispatch

Query-index sorting uses stable four-bit digits. SIMD prefix sums count entries
within each digit; group offsets preserve the original row order. The compiler
also routes a seed row only to rules with a compatible first branching table.
Grounded queries still execute once, including on an empty graph. Rules with
several seed types keep all variants; final binding order and RHS remain unchanged.
`--radix-digit-bits 1` and `--flat-dispatch` preserve the old execution variants.

The same 3,865-Run suffix from checkpoint 6,232 has byte-identical final device
states for these changes (699,784 rows):

| Variant | GPU prepare | GPU match | GPU commit/rebuild | GPU total |
|---|---:|---:|---:|---:|
| Index all columns, binary radix | 46.01 s | 15.02 s | 13.86 s | 74.88 s |
| Index used columns | 17.16 s | 14.44 s | 13.91 s | 45.50 s |
| Four-bit stable radix | 10.01 s | 14.51 s | 13.71 s | 38.23 s |
| Dispatch by seed type | 9.79 s | 4.94 s | 13.60 s | 28.34 s |

These are backend improvements, not Cell-specific credit or a complete Pass 3
speedup. The original all-column control was a diagnostic run with periodic
snapshots (copies excluded from GPU phase times). The last three are clean suffix
timings. Evidence: `used-index-results.json`, `radix-results.json`, and
`seed-dispatch-results.json`. The current regression suite has 48 checks,
including the stable/unstable RHS cases below. A separate snapshot/extraction
suite has 10 checks.

## Interpreting selected programs

`check_selected_execution.py` loads the actual production Tiger output through
eggcc's `FromEgglog`, preserving shared `Rc` identities, then executes it with
eggcc's `interpret_dag_prog`. It does not implement a replacement evaluator. The
test driver injects state parameters from the function signature and accepts
primitive JSON input arguments; pointer inputs/returns require a different
observational checker and are rejected here. Returned float bits and print logs
are compared exactly.

```sh
python3 experiments/device_backend/check_selected_execution.py \
  --eggcc /path/to/check_statewalk --build out/selected-vm \
  --target /path/to/check_statewalk/target \
  --library-path /opt/homebrew/opt/cbc/lib \
  --before native-selected.egg --after gpu-selected.egg \
  --inputs experiments/device_backend/fixtures/raytrace-inputs.json \
  --output out/selected-execution
```

The first input is the benchmark's official arguments; the other three vary the
scene parameters. `selected-execution-results.json` records agreement for all
four, despite nine structurally different Function roots. These tests **do not
replace** the failed strict DAG comparison, prove general equivalence, measure
generated-machine-code quality, or establish a backend speedup.
The original `raytrace` main prints only one scalar, not the rendered screen;
the four checks therefore do not validate every computed pixel.

## Stable empty RHS proofs

Before ordered submission, the GPU may prove that a binding's entire RHS is
empty **and stays empty for the rest of this action phase**. It can reuse an
existing constructor (including hidden rows), an already-true equality, an
already-hidden subsumption, or a non-improving min/max update. Arithmetic must
succeed. Mutable function reads, allocating set operations, panics, and
old/new/custom merge policies are not certified. Seminaive fact-free one-shot
initializers do not get unrolled proofs.

This depends on deferred unions: canonical keys cannot move while the ordered
RHS batch is submitted. Hiding is monotone, an existing equality remains true,
and min/max can only move farther in the direction that makes the certified
write redundant. Unsupported or unproven cases execute their original RHS.
Logical match counts, watermarks, source scheduling and the order of remaining
actions are unchanged. This is a generic backend optimization, not Cell proof
credit or sat-skip.

Proofs are compiled into direct scalar code and use the existing rule-sorted
binding permutation, so adjacent GPU lanes run the same program. They share
the RHS command buffer. `noop_preflight_seconds: null` means no independent
timestamp was taken; `rhs_commit_includes_preflight` marks that inclusion per
command. `parallel_rebuild` is measured separately. `--no-noop-preflight` is the
control. The unsuccessful bytecode version and its regression remain in the
performance record, rather than being presented as an optimization.

`full-pass3-results.json` records single clean runs from the original input,
with no CPU rule execution. The source schedule has 10,097 Runs:

| Execution | Complete inference | Notes |
|---|---:|---|
| Current local native kernel | 23.11 s | Historical unchanged control; not pristine upstream |
| GPU, used indexes + four-bit radix + seed dispatch | 64.21 s | Complete fresh execution |
| GPU, bytecode no-op filter | 103.02 s | Rejected implementation; extra work outweighed savings |
| GPU, direct compiled proofs fused with RHS | 54.65 s | 1,539,006 empty RHS submissions skipped |

The last GPU result has exactly the same final device state as its 64.21-second
control. It is **still slower than native**. Its 54.65 seconds include host
schedule/command wait; device time is 45.61 seconds. Startup was 0.94 seconds
with a warm Metal compilation cache, reported separately. Cold compilation has
taken around 70 seconds and must not be mistaken for this warm startup result.
The proofs are not matrix/Tensor Core execution, and the backend does not yet
load automatic Cell/ridge plans.

`compare_runs.py --device-states --endpoints-only` checks the identical original
typed program, executed schedule and final state across a fresh execution and a
continuation, without a timing ratio. Only unused slots beyond each table's
declared row arity are ignored; visibility, timestamps, fact IDs, all declared
values, sets and equality classes are still compared.
