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
consumer of that target remains pending. Known certified alias recipes can now
select a distinct target template and project its own slots directly on GPU.
The original match evidence is retained. The standalone validation performs first-time Bake offline. The new
[`matrix-live` loop](../matrix-live/README.md) can Bake and hot-install unknown
constructor specializations while retaining the completed match.

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

The prior integration passed 47 package tests with the numerical workers enabled, including the
34 pre-existing runtime checks. The resident controls used a worker built from
the committed source; the unrelated working-tree edit to `resident.swift` was
left untouched.

The original eight coordinate tests cover deferred depth, OR-branch separation, exact swapped/repeated
pair markers, stale source partitions, successful matches needing specialization,
incremental feature/evidence invalidation, invalid wiring and generated layouts.
With `CELL_MATRIX_WORKER`, numerical/state results are checked against real MPS.
Without it the tests run only the CPU reference.

## Remaining roadmap work

This is a bounded compiled Cell program, not automatic matrix lowering of any
`.egg`, and not yet the main `analyze → native runtime` execution path. The adapters
use the existing finite Bake/interface and supplied-boundary contracts. They do
not discover FractalComb from source syntax or prove unobserved transfer domains.

Still outstanding: a general analyze frontend beyond the supported two-read
contracts and the sharing/transfer certification split; hot installation of unseen
alias cases and numeric specialization in this executor;
real native-event-driven routing and global commit synchronization; FractalComb
program compression from analyze evidence; checked numeric update instructions
in this IR; and full real-eggcc same-state performance tests. The existing resident
hash backend and i64/function tests remain controls, not claims that these gaps
have been solved by renaming storage to matrices.


## Analyze evidence and certified specialization continuation

The shared executable now accepts `--mode matrix --analysis DIR` instead of
`--program FILE`. `src/analyze_program.rs` imports **only witnessed connections**
from `cell-egraph.json.repetition_families.port_progressions`, plus `history.json`
and the exact completed `catalog/states/` certificates. It checks:

- Typed port identities, parameter marker positions, exact alias frames.
- Actual committed producer facts, consumer read versions, both full LHS atoms.
- All logical variable equalities, including repeated occurrences within an atom.
- Equal rule environments and complete visible parent-body transport into the target.
- Complete target gathers and the observed dependency order.

The source rule AST is parsed with egglog only to validate those existing
witnesses. It does not discover families. Additional guards, computed/literal
reads, cross-atom result equalities without an exported coordinate, incidental
alias quotients, unversioned target cycles and missing certificates are explicit
unsupported cases. They cannot silently vanish into a supposedly complete plan.
The original analyzer report remains unchanged; import exclusions are included
in the generated program. If nothing qualifies, compilation fails with reasons.

Freshly analyzing `experiments/cell_egraph/port-progression.egg` produced three
source instances and two target instances, sharing two certified templates.
The two witnessed boundary channels compile to one 4-by-4 Gram block (32 logical
MACs). Both CPU and MPS return the same two target bindings. A separate native
query checks the positive connections and a broken-connection negative case.
This is a finite observed-edge result, **not** full future Cell coverage or an
infinite FractalComb proof.

```sh
EGG_LAYOUT_GRAPHS=0 target/analyzer/debug/egg_layout analyze \
  --source experiments/cell_egraph/port-progression.egg \
  --output out/matrix-analyze --save-history
mkdir -p out/matrix-run
swiftc -O cell-assisted-egglog/matrix.swift -o out/matrix-run/matrix-metal
cargo run --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted -- --mode matrix \
  --analysis out/matrix-analyze --backend metal \
  --gpu-worker out/matrix-run/matrix-metal --output out/matrix-run/report.json
```

This emits `report.program.json` for offline replay and `report.lowered.json`
with generated matrix layouts. Source IR remains version 1; the lowered worker
ABI is version 5. Old lowered files must be regenerated from their source IR.

A target can now carry **offline-certified alias alternatives**. Each alternative
has an exact whole-frame equality partition, a target template, and a projection
from the gathered frame into that template's compact parameters. The matrices
for these guards and projections are generated by the same compiler. Alternatives
must be disjoint, have unique fresh target slots, and agree with their target
partition. A certificate field records provenance; JSON metadata by itself is
not a proof. The trusted offline compiler must establish the transfer contract.

For the tiny math workload, `observed_alias_specializations` reuses the existing
constructor-only environment validator and native Bake. It specializes the
**complete retained parent seed**, never just the rewrite RHS. Two observed
alias cases produce 38-node / 15-class closures, each exactly equal to a separate
native run. GPU recipe selection then enables two further native-checked
factoring bindings; the resulting target closures each have 63 nodes / 23 classes.
The three stages have 6, 4 and 2 distinct native full bindings. An unchanged
second epoch uses zero GEMMs and computes zero feature rows; guard/gather and
control work still runs. No runtime speedup is inferred from these tiny counts.

`target_status` is 0 (no published target), 1 (original template), 2 (matched but
needs an unknown specialization), or 3 (a certified alternative was published).
`specialization_targets` records the chosen target. A rejected match invalidates
all its alternative outputs, so a stale specialized Cell cannot enable consumers.
The original incompatible target stays unavailable; consumers explicitly use the
specialized frame. Unresolved sources remain pending.

First discovery in this experiment performs native Bake and recompiles the
program. **Already-installed** recipes run without host binding readback or
repeating the completed match. Unseen alias cases are not learned inside Metal;
general union repair, numerical conditions and hot resume remain future work.

```sh
python3 cell-assisted-egglog/validate_matrix.py --skip-build \
  --analysis out/matrix-analyze --output out/matrix-validation
```

The validation includes all earlier marker/depth/whole-closure checks, both alias
cases, post-specialization matches and direct analyze import. Relevant results:
[analyze-specialization-results.json](analyze-specialization-results.json).
The checked-in `tests/data/analyze_ports.json` is a trimmed real analysis fixture;
no runtime result from that fixture is uploaded as a matrix match answer.


Live continuation is now available separately through `--mode matrix-live`; see
[the native event loop](../matrix-live/README.md). The preceding offline experiment
remains reproducible. Its old statement that hot resume was future work describes
that experiment's boundary, not the newly implemented runtime mode. Numeric and
general effectful specialization remain unsupported by the live matrix compiler.


## Typed numerical dataflow

`Program.numeric` now carries exact i64 expression columns, guards, state reads,
and masked min/max proposal reductions in the same generated matrix allocation.
Numeric conditions participate in the existing depth classifier; following stages
can consume merged state. The native function-update path is `--mode matrix-numeric`.
See [the numeric contract](../matrix-numeric/README.md) for supported operators,
error/commit behavior, remaining CPU work and exact native comparisons. It does not
relax the constructor-only assumptions of automatic closure Bake.
