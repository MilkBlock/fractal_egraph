# Rule-derived Cell kernel: CPU, Metal, and native fact submission

> Automatic shared long-range touch discovery: [method and checks](TOUCHPOINTS.md).
> Current architecture: [one shared Program for observe/update/code generation](SHARED_PROGRAM.md).
> Verify with `python3 experiments/cell_kernel/test_program.py` and
> `python3 experiments/cell_kernel/verify_shared.py --gpu`.
> The timings below are historical results from commit `3952781`; they were not
> rerun for the shared-program revision and do not measure its performance.

2026-10-09. This is a complete **fixed-shape pilot**: derive an affine/min plan
from two actual eggcc rules, emit CPU/Metal code, execute a batch, and submit its
analysis/selection facts back to the actual local egglog engine. No egglog engine
source was changed. It is not yet connected to production analyze/bake, and is
not the whole CICM optimization or an eggcc program benchmark.

## Reproduce (macOS with Swift/Metal)

```sh
cargo build --offline --release --example cell_kernel_oracle
python3 experiments/cell_kernel/run.py --build --count 1024 --output out/cell-kernel-run
python3 experiments/cell_kernel/test_backend.py --gpu
```

The generated Swift CPU/Metal driver is built before measurement. Metal hardware
access may require permission outside an execution sandbox; device failure is an
error, never silently replaced by CPU. `--devices cpu` runs only native and CPU.
No network, trace capture, analyze, or bake is needed. An existing check_statewalk
checkout supplies the actual rule source. The compiler fails if its supported
size rules or the previously verified CICM LHS change.

## What is compiled

Source: `check_statewalk/dag_in_context/src/utility/expr_size.egg` Bop and Uop
rules, plus the first Uop LHS of conditional_invariant_code_motion.egg.

The supplied Cell shape is:

    left  = Uop(Bop(a,a))
    right = Uop(Bop(b,c))

Boundary sizes a,b,c and old root minima are input. Prior Bop sizes must be absent.
Only these two size rules are loaded; this is NOT full Expr-size analysis, which
would derive additional boundary values. Leaf is an explicit fixture placeholder
for already-analyzed boundary expressions. Type/purity/context/extracted-Term
facts are supplied, not recomputed by the pilot.

The exact lowered arithmetic is:

    bopLeft  = 2*size(a)+1
    bopRight = size(b)+size(c)+1
    left     = min(oldLeft, bopLeft+1)
    right    = min(oldRight,bopRight+1)

The coefficient 2 arises from the repeated Bop child; the source computes tree
size, not DAG size. The generated plan contains matrix rows [2,0,0,0] and
[0,0,1,1], then min with old root sizes. Both Swift CPU arithmetic and Metal source
are emitted from those coefficients. The native rules and an independent Python
reference provide separate checks. This is a small source-checked specialization,
not a general egglog-to-GPU compiler or SVD-based discovery.

Selection preserves the source's strict <10 tests and shared Term/operator/base
type/predicate/inputs plus purity and branch tuple-type conditions. Costs remain
separate (they need not be equal). Unique frame contexts and a single left/right
candidate per instance prevent cross-instance or additional pair bindings. The
baseline keeps the full original LHS and adds the owning Instance fact. It
collects all 18 named binding fields. Native checks verify every selected full
binding and every rejected instance; cardinalities agree.

## Execution paths

    native: setup -> Bop/Uop saturation -> original LHS selector -> result rows

    CPU/GPU: setup description + binding records -> fused batch kernel
             -> Computed TSV relation -> short id-keyed commit rules
             -> actual native size rows, Selected and FullBinding -> result rows

The external path commits BOTH Bop and Uop sizes (four targets per instance).
It does not omit internal size effects to create artificial savings. The commit
rules still perform keyed joins to Instance/KernelPayload; computation is not
claimed to be entirely join-free. KernelPayload and Computed are additional
relations whose setup/ingestion overhead is included. No virtual-fact or hashcons
implementation is changed.

Each native oracle invocation runs setup and actions in one EGraph. The three
variants use separate equivalently seeded graphs. Full equality checks run AFTER
the measured action phase; all validation time is separately reported.
The GPU/CPU driver currently exchanges files and starts a new process; no hidden
persistent-device/zero-copy speedup is assumed. Metal uses shared buffers and
explicit command completion before results are submitted, so there is no race
with concurrent graph mutation. General concurrent union/subsume is not handled.

## Measured result: one run of the generated version, 1024 instances

All three variants returned exactly the same:

* 1024 five-column KernelResult rows (id and four size values).
* 211 selected full bindings, all source variables checked.
* 7168 Expr-size table rows (three boundary, two Bop, two Uop per instance).

Actual GPU: Apple M3 Max. The engine is the current LOCAL MODIFIED egglog checkout,
not pristine upstream. All variants use the same newly built oracle binary;
its hash and the backend hash are recorded in results.json. No averaging.
An earlier fixed-reference development run is retained only under out/; this
report is the source-generated implementation's single recorded run.

| Stage (ms) | Original rules | Fused CPU | Metal GPU |
|---|---:|---:|---:|
| Native inference or external-result submission | 19.737 | 4.068 | 4.006 |
| Backend process including launch, I/O, setup | 0 | 8.968 | 45.658 |
| Backend compute only (nested within previous row) | - | 0.0067 | 5.227 |
| GPU device execution (nested within compute) | - | - | 0.0127 |
| Native setup execution | 932.202 | 988.315 | 1021.067 |
| Whole driver, including validation | 1780.906 | 1812.264 | 1927.399 |
| Whole driver minus separately measured validation | 1065.116 | 1113.696 | 1187.147 |

Rows overlap and must not all be summed. The last row is a subtraction, not a
separate timing run. Full driver includes source generation, file I/O, child
startup, parsing, setup, GPU setup where applicable, submission/rebuild, output
inspection and reporting. One-time Swift/Rust toolchain builds are excluded.
Shared fixture/plan-generation time is reported separately. System Metal compiler
caches were not cleared. The GPU process still creates a new device queue/pipeline;
setup=33.128ms, kernel compilation within setup=1.239ms on this recorded run.

**No end-to-end speedup was achieved.** The rule execution slice shrank, but the
CPU kernel is only ~7 microseconds. GPU dispatch/synchronization and startup
outweigh its arithmetic. Building the extra interface facts also costs time.
These results motivate persistent outer-runtime integration and larger fused
work, not claiming that tiny affine Cells already benefit from GPU.

## Validation and artifacts

`test_backend.py --gpu` checks 19 exact rows including near-2^30 input values,
strict size9/10 behavior, old minima, aliasing, flags, mismatched keys, and CPU/GPU
byte-equivalent outputs. Four malformed inputs must fail; empty CPU input works.
The uint32 Metal arithmetic is exact within this declared range; arbitrary
native i64 overflow semantics are NOT compiled. An unsupported input is rejected.

Generated files under the chosen output directory:

* plan.json, kernel.metal, backend.generated.swift
* native/cpu/gpu setup.egg, actions.egg, checks.egg
* computed.tsv and binary input/output for external backends
* native.json with actual committed table rows, timings, and validation status
* report.json with end-to-end timings and equality checks

No generated binaries or data streams are tracked. The reusable source files are
run.py, backend.swift, test_backend.py, and examples/cell_kernel_oracle.rs.

## Deliberate limits

Instances are supplied by the fixture, not automatically discovered from Cell
history. This experiment does not solve optimal fragment selection or the cost
of constructing bindings from arbitrary production graphs. Full CICM RHS code
motion, helper Subst evaluation, union/subsume effects and extraction are not
executed here; the earlier fresh-create/subsume issue remains a separate blocker.
Selected is a selector result, NOT a completed CICM transformation. This pilot
therefore verifies only the declared size-analysis/selection compilation cut.
