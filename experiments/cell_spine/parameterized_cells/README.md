# Parameterized Cell repetition

This standalone experiment extends Cell specialization from **copying opaque
input slots** to **guarded numeric parameter updates and child-Cell connections**.
It does not modify egglog's engine or claim an eggcc speedup. The existing math
Cell/moment backend remains a separate, explicitly reported regression case.

## What is extracted

`lhs_ast` uses egglog's parser. `compiler.py` reads its typed declaration index
and surface rewrite AST. It finds constructor roots with numeric arguments,
resolves local definitions in conditions, and extracts:

```
source family and parameter roles
    + guard expressions
    + shared arithmetic DAG
    + RHS constructor tree and each child-family position
    -> next parameter values and retained constructor/union effects
```

The compiler does not contain Fibonacci formulas or a whitelist of rule names.
It supports numeric-root constructor rewrites with variable LHS slots, integer
literals, binary `+`, `-`, `*`, comparisons and equality definitions. The full
source RHS is preserved, not only its recursive calls. A child at RHS path `[0]`
and one at `[1]` remain separate connections even when their parameters agree.

For the included rules, it extracts six guarded templates in three families:

```
Fib(n), n > 1: Add(Fib(n-1), Fib(n-2))
Fib(n), n <= 1: Num(n)
StepA(n), n > 0: StepB(n-1)
StepB(m), k=m-1 and k>0: StepA(k-1)
StepA(n), n <= 0: Num(n)
StepB(m), m <= 1: Num(m)
```

`compose()` substitutes actual child parameter expressions into the next rule's
guards and updates. Thus `StepA -> StepB -> StepA` yields `n-3`, guarded by both
`n>0` and `n-2>0`. It retains the intermediate StepB effect. The affine matrix is
an analysis view; execution retains the source arithmetic order and checks range.
The family dependency graph automatically identifies `{Fib}` and `{StepA,StepB}`
as recursive components. This is **not** a proof of termination or infinitude.

## What Bake and execution do

The source AST provides candidate programs. Native logical match probes on the
Bake input identify which rule programs have actually been witnessed. Only those
programs are admitted on later inputs. An applicable but unobserved program is
reported as `pending: unbaked-template`, not silently rejected as a failed match.
These probes record logical matches; they do not count committed mutations.

At runtime an instance key is `(family, numeric parameter tuple)`. Fib(5) and
Fib(8) share code, **not** a saturated state or e-class. Existing child keys are
reused, so multiple parents can point to Fib(3) without generating duplicate work.
Every matching alternative fires; the runtime never chooses just one RHS.

The Metal worker computes guards and updates with exact checked int32 arithmetic.
It accepts i64 source fields only in this supported device range. Overflow is a
third outcome (`pending`), distinct from a false condition; it cannot publish a
completed Cell. A false guard avoids evaluating the RHS. There is no float
conversion of numeric IDs and no silent wrapping. Comparisons, parameter updates
and child-field output are real GPU computations, not CPU-computed answers
uploaded for confirmation.

The CPU supplies each frontier, deduplicates `(family, parameters)`, retains the
effect log and routes the next frontier. The Metal process/device/pipeline stay
alive across batches. This is not a device-resident worklist or a GEMM throughput
experiment. The numeric kernel interprets shared DAG tapes; the existing math
backend uses Metal/MPS matrix operations for its different boundary domain.

There is no implicit recurrence depth budget. The bundled examples terminate.
Calling `explore()` on an unbounded increasing recurrence is not a safe generic
execution policy; it must first get a termination argument or a caller-controlled
interruption mechanism. No finite observed prefix is labeled saturated.

## Independent verification

Three batches use the **same program**:

| Input | Distinct family instances | Logical matches | Child references |
| --- | ---: | ---: | ---: |
| Bake: Fib(5), StepA(6), StepB(6) | 16 | 16 | 16 |
| New values: Fib(8), StepA(11), StepB(10) | 17 | 17 | 21 |
| Shared: Fib(9), Fib(8), Fib(5), StepA(3), StepB(2) | 13 | 13 | 18 |

The native observer evaluates the **original** guard and RHS expressions,
independently of the lowered arithmetic DAG. It checks every complete input/local
binding and every numeric RHS field, including repeated fields at different paths.
For each batch, CPU and Metal results agree exactly.

We then materialize all emitted source/RHS facts and unions into a fresh native
egraph with **no recursive rewrite rules installed**. Its whole constructor graph
and e-class partition must equal native execution of the original rules. This is
checked both before and after native Add/Num constant folding. Folding is residual
native work in this prototype; the GPU does not compute Fibonacci outputs alone.
Residual rewrites that could create new untracked numeric-family terms are rejected.

The accompanying original math test audits 6 + 4 bindings in two stages, and
another 6 + 4 after changing inputs, including known alias-pending cases. Both
Metal/MPS FP32 and FP16-input modes match its reference. This is a regression of
the existing path, not evidence of a unified general GPU runtime.

Additional tests cover two parameters, duplicate child references, alpha-renamed
rules, local definitions, overlapping enabled alternatives, unknown primitives,
cyclic definitions, missing Bake certificates, numeric equality, overflow, and
990 GPU arithmetic/comparison cases including integers beyond 2^24.

## Reproduce

```sh
python3 experiments/cell_spine/parameterized_cells/demo.py \
  --output out/parameterized-cells-new

# Reuse already built local Rust examples:
python3 experiments/cell_spine/parameterized_cells/demo.py \
  --output out/parameterized-cells-newer --skip-build

python3 -m unittest experiments.cell_spine.parameterized_cells.test_families -v
```

Use `--cpu-only` without Metal; `--skip-math` runs only the new numeric analysis.
The output directory must be new. It contains `index.html`, `families.json`, the
instance DAG/effects per batch, and the independent native evidence. Compact text
results are versioned; executables and full generated exports are not.

## Boundaries

This is parameterized rewrite-family analysis with explicit retained effects,
not general synthesis of RipSatCell equivalence classes. It does not infer the
weakest guard, arbitrary slot symmetries, hidden e-class union updates, generic
function/set effects, runtime neighbor discovery, or complete rule coverage from
finite Bake examples. Numeric parameters are not erased or folded into opaque
identity partitions. Ordinary residual native computations and the original math
Cell backend are visibly separated from the new numeric device path.
