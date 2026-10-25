# GPU-owned egglog backend

The CPU keeps native parsing, typechecking and schedule control. Metal owns the evolving tables, matching, RHS actions, union-find and rebuild. Normal execution returns status/timing data, never bindings for CPU-side RHS execution.

This remains an experimental, mostly serial device backend. It has **not completed the full raytrace Pass 3 schedule or extraction validation**, and has no full Pass 3 acceleration claim. Explicit Cell/spine/marker `.egg` programs run through the same backend; automatic Analyze library loading, the ridge predictor and Tensor Core matching are not connected here.

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
- `--resume DIR`: restore the last complete native-verified checkpoint, then continue the original schedule. `--max-runs` is an absolute schedule count, including the restored prefix.
- `--verify-against DIR`: reuse a saved, checked native endpoint for the identical core program and schedule. No native Runs are executed. This is useful for controlled ablations from the same checkpoint.

Source `check` assertions always execute on GPU, including when native verification is off. The native frontend compiles them into isolated helper relations; private/helper sorts are included on both comparison sides.

```sh
MTL_DEBUG_LAYER=1 python3 experiments/device_backend/test_backend.py \
  --build out/device-build --output out/device-tests
python3 experiments/device_backend/test_export.py --build out/device-build
```

For phase logs set `EGG_DEVICE_PHASE_LOG=1`. Diagnostic ablations: `--eager-unions`, `--rebuild-query-index`, `--no-delta-queries`. Eager union is a legacy counterfactual with a known semantic mismatch, not a generally valid alternative executor.

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

Each ruleset has a match entry and shares prepare/commit kernels. These stages have separate Metal command timing. RHS is fixed-width bytecode in GPU-private storage, with one device register file for the current binding. The large initialization RHS is not unrolled into thousands of Metal statements.

Matching finishes before any RHS write, so the logical snapshot aliases the row buffer safely; an entire copy is unnecessary. Query indexes persist across Runs. Append-only changes extend chains; existing value changes, visibility changes and union/rebuild invalidate them. Keys already fully bound use direct complete-key lookup.

Seminaive rules partition matches by the first table atom whose row timestamp exceeds the rule watermark. Earlier atoms must be old; the pivot is new; later atoms are unrestricted. These branches are disjoint, including when multiple atoms change together or a row participates in a self-join. Timestamp buckets locate pivot rows without scanning an entire table. Naive rules keep unrestricted matching. Per-rule watermarks remain shared across direct and combined rulesets.

RHS union is queued until the action phase ends, matching native union-table insertion semantics. Immediate canonicalization can suppress a required merge constructor; `deferred-union.egg` retains that regression. This is not merely an ordering performance option.

Complete-key writes use a GPU hash index. Rebuild canonicalizes rows, freezes duplicate-key groups, performs merges and repeats if unions/new rows require another round. It preserves subsumption and fact redirects. A no-union step needs no congruence rebuild.

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

Normal Run traffic is 8 command bytes and 68 response bytes (status, three phase durations and index rebuild count). Initialization/program upload and diagnostic snapshots are additional. No intermediate binding list is downloaded. Reports distinguish:

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
