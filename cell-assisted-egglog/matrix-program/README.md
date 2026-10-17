# Shared Cell matrix program

This implements the first executable slice of `MATRIX_PLAN.md`: one IR joins
existing coordinate/marker, staged matching and target-gather work. It is a mode
of the `cell-assisted-egglog` executable, not another rule-family detector.

## One program, two executors

- `src/program.rs`: Cell template descriptors and binding rows; the shared
  port/lift2/mean/concat/moments feature DAG; whole-binding OR alternatives with
  condition depths; target gathers and input-partition guards; input updates;
  validation and an independent integer CPU reference.
- `src/compile.rs`: derives matrix shapes, typed columns, offsets and Metal ABI
  constants. Swift and Metal do not duplicate a hand-maintained stride array.
- `matrix.metal`: demand, feature maintenance, three-state classification,
  compaction, exact Gram-distance decisions, target construction and invalidation.
- `matrix.swift`: executes **Metal/MPS matrix multiplication**, using the same
  generated storage. It does not search egglog tables or interpret binding truth.
- `tools/cell_spine/matrix_program.py`: imports the existing baked layered/marker
  and two-stage transition programs. It shares their symbolic wiring; it does not
  upload their already-computed match answers.

The immutable template descriptions retain the parents' context. Instance rows
hold their template identity and parameter binding. They are not copies of the
expanded Cell egraph. A returned row is meaningful only according to its validity;
inactive rows may retain old cache bytes for reuse.

```text
baked interfaces + actual/offered external connections
    → shared coordinate DAG + marker + binding alternatives + target maps
    → initial Cell matrices
    → request only unknown conditions of surviving branches at this depth
    → compact matrix rows on GPU
    → build only needed, invalid feature coordinates
    → MPS Gram block → exact condition results
    → complete / rejected / continue
    → target gather + target partition check
    → next stage consumes the target coordinates in the same GPU storage
```

A complete binding with an unsupported target alias pattern is preserved as a
**needs-specialization target**, distinct from rejected matching. A downstream
consumer of that target remains pending. Automatic native re-Bake and resumption
of a newly compiled specialization are not connected in this slice.

The feature algebra is migrated from `moment_compiler`. Signed-bit IDs use no
float interpretation of the identity itself. The binary-pair moment codec keeps
multiplicity and swap symmetry, including `a+a`. It is not a whole-leaf bag codec
for arbitrary trees. The compiler checks the exact FP32 integer range; unknown
ops, missing coordinates, unsafe layouts and invalid stage dependencies fail.

## Where host work remains

The GPU makes branch decisions and selects required rows. MPS needs matrix sizes
when encoding a command, so the host reads **one compacted row count per candidate
block** and submits the corresponding MPS operation. It does not read intermediate
bindings or feature matrices to schedule those operations. Diagnostic full-state
snapshots are copied on-device and read only after the whole program finishes.

This is not a fully device-driven/one-submission scheduler. Group compaction is
currently one GPU thread per group, and shared metadata is scanned. Count-read
synchronization, command submissions, padding and buffer memory remain costs.
A program invocation keeps the matrices across all stages and input-update epochs;
there is no persistent native-event service attached to this executor yet.

Feature and condition validity persists across epochs. Changed input slots
invalidate dependent coordinates. Unchanged marker coordinates and evidence are
reused. Derived target validity is reestablished each epoch, so an old successful
target cannot authorize a new match when its producer now fails.

Matrix MAC counts use the dimensions actually passed to MPS, including unrequested
positions within each Gram block. They are logical GEMM operations, not hardware
instruction counts: opaque MPS tile padding is not inferred. Feature work is
reported separately. Physical matrix bytes include allocated row padding and
static program storage; they are not whole-process RSS. No acceleration claim
against native egglog or fraction of neural-network throughput is made.

## Verification

The validation adapter freshly Bakes the old tiny math inputs with the native
helper, then feeds the same generated IR to the Rust CPU and Metal/MPS backends.
It imports both the old leaf-wiring plan and the Pair-marker plan; the old
matcher/reference is used only for separate validation, not runtime execution.

| Imported workload | Logical matrix MACs | MPS calls | Computed feature rows |
|---|---:|---:|---:|
| root, all depths at once | 3,136 | 1 | 28 |
| root, staged | **896** | 3 | 24 |
| root, staged + Pair marker | 912 | 3 | 50 |
| all root/non-root, all depths at once | 3,136 | 1 | 28 |
| all root/non-root, staged | 3,136 | 1 | 28 |
| all root/non-root, staged + Pair marker | 3,776 | 2 | 52 |

Pair markers resolve different information earlier, but **do not improve these
MAC counts over the new signed-bit staged plan**. Marker cost is retained: these marker cases also perform 80 scalar additions
and 176 scalar multiplications in feature construction, reported separately
from GEMM. The
old Hadamard-based experiment is not used as a timing or same-codec cost baseline.

- All variants agree on every binding channel. The native oracle confirms **462
  port-tagged complete factoring bindings**, including non-root positions, over
  the fixture's supplied Add contexts. This is not 462 new mutations or a claim
  that all those outer Add contexts existed in the original unaugmented input.
- The shared IR also executes the old two-stage transition program: **6 + 4**
  native bindings, including matched-but-alias-pending targets, and **8 exact
  native whole-closure checks** for published targets. Targets feed the next stage
  through the same device matrix storage.
- The RHS-only negative control remains: the power target retains **13 nodes /
  9 classes**; keeping only its RHS yields **9 / 7** and is not an equivalent
  future matching state.
- An unchanged epoch performs **zero feature recomputation and zero GEMM**.
  A payload-only update changes target bindings without new matching GEMMs;
  changing a participating factor recomputes the affected evidence. Updated
  results equal fresh execution and have a separate native closure/binding audit.

`results.json` records counts and native checks. The actual source/native helper
hashes are retained. The validation's output directories also contain generated
IR, generated matrix layouts/shader, full CPU/GPU results and native audit inputs.
Those generated intermediates and binaries are not committed.

## Reproduce

From this worktree root (offline, macOS Metal):

```sh
python3 cell-assisted-egglog/validate_matrix.py --output out/matrix-run
```

After building the helpers, `--skip-build` avoids rebuilding Rust. Run a generated
program directly, without the Python validation adapter:

```sh
cargo run --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted -- \
  --mode matrix --program out/matrix-run/root-marker.json \
  --backend metal --gpu-worker out/matrix-run/matrix-metal \
  --output out/matrix-run/replayed.json
```

Use `--backend cpu` for the integer reference. Unknown IR fields are rejected;
this entry does not reinterpret unsupported numeric/effect operations as equality.

```sh
CELL_MATRIX_WORKER="$PWD/out/matrix-run/matrix-metal" \
  cargo test --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted --test program
```

All 42 package tests pass with the numerical workers enabled, including the
34 pre-existing runtime checks. The resident controls used a worker built from
the committed source; the unrelated working-tree edit to `resident.swift` was
left untouched.

Eight new tests cover deferred depth, OR-branch separation, exact swapped/repeated
pair markers, stale source partitions, successful matches needing specialization,
incremental feature/evidence invalidation, invalid wiring and generated layouts.
With `CELL_MATRIX_WORKER`, numerical/state results are checked against real MPS.
Without it the tests run only the CPU reference.

## Remaining roadmap work

This is a bounded compiled Cell program, not automatic matrix lowering of any
`.egg`, and not yet the main `analyze → native runtime` execution path. The adapters
use the existing finite Bake/interface and supplied-boundary contracts. They do
not discover FractalComb from source syntax or prove unobserved transfer domains.

Still outstanding: direct analyze evidence import and the sharing/transfer
certification split; automatic alias/numeric specialization in this new executor;
real native-event-driven routing and global commit synchronization; FractalComb
program compression from analyze evidence; checked numeric update instructions
in this IR; and full real-eggcc same-state performance tests. The existing resident
hash backend and i64/function tests remain controls, not claims that these gaps
have been solved by renaming storage to matrices.
