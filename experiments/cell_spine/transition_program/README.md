# GPU Cell matching, construction, and continuation

This standalone experiment extends `moment_compiler` from interface comparison
to a **two-stage Cell transition program**. It uses four original math rewrite
bodies: factoring, power combination, `Diff x (Sin x)`, and `Integral (Cos x) x`.
The small Bake environment also contains Add/Mul commutativity and distribution.

Both stages are encoded into one Metal command buffer. GPU results determine
which targets are valid; the next stage reads those target inputs directly.
No CPU match search, result-dependent scheduling, or intermediate host readback
occurs between the stages. Device snapshots are collected for later verification.

This is **not** a general GPU egglog runtime. The experiment supplies the external
connections and a finite two-stage schedule. Native egglog still performs Bake.

## What is generated automatically

The input is the selected `.egg` source, a few parameterized source Cells, and a
list of offered boundary connections. Rule ASTs come from egglog's own parser.

For each connection the compiler:

1. Matches each side of the rule against the corresponding **symbolic baked**
   source Cell, retaining variable bindings and alternative e-nodes.
2. Extracts shared-variable conditions and compiles their Input/Add interfaces
   with the exact moment coordinates. Different rules use common condition
   matrices whenever they request the same numerical wires.
3. Uses the proved equality of a shared interface to substitute that whole
   subexpression into the second parent's seed. This avoids choosing a leaf
   permutation merely to represent an unordered Add pair.
4. Retains **both entire parent seeds plus the new boundary constructor**, not
   just the rewrite RHS. It assigns parameter slots to the retained context.
5. Bakes that context and checks that native saturation derives the original RHS.
6. Emits a fixed target-input gather and its input-partition guard.

Step 1 runs during compilation, not for each concrete runtime binding. This
compiler accepts only the selected positive constructor rewrites and the certified
Input/Add common-interface domain. It is not an arbitrary `.egg` frontend.

The supplied schedule describes possible connections, not their successful
bindings. A held-out variation changes inputs and reverses which factoring path
succeeds, using exactly the same compiled program and baked templates.

## Runtime representation

Each reserved Cell row has:

```text
template identity: fixed by the compiled transition
input coordinates: slots × signed binary bits
validity:          one byte
```

Common template/interface descriptions are shared. The target's full egraph is
implicit in its template and input coordinates; it is not generated as physical
GPU e-nodes. The reference renderer can materialize the template to verify it.

For every stage:

```text
Cell inputs → feature circuit → shared GEMMs → exact conditions
                                                   ↓
source validity + condition results → input gather → target partition check
                                                   ↓
                                      target inputs + validity
```

The transition kernel has three outcomes:

| Status | Meaning | Can the next stage consume it? |
| --- | --- | --- |
| 0 | A source is invalid, or a condition fails | No |
| 1 | Match succeeds and the baked target guard holds | Yes |
| 2 | Match succeeds but the target needs alias specialization | No; retained as pending work |

Status 2 is not presented as a failed rule match. The original rule is applicable;
the selected target representation is not certified for those input aliases.
Automatic specialization/retry is deliberately not implemented in this experiment.

Outputs occupy fresh preallocated rows. Sources must precede targets, and no
transition may overwrite an initial row or another target. This prevents an
invalid/padded row from acquiring a valid successor accidentally.

## Why keeping only the RHS is wrong

The power rule is one-way:

```text
Mul(Pow(k,b),Pow(k,c)) → Pow(k,Add(b,c)).
```

Starting from the RHS alone does not recreate both old Pow nodes. In the concrete
fixture, the retained closure has **13 e-nodes / 9 e-classes**, whereas RHS-only
has **9 e-nodes / 7 e-classes**. They may represent an equivalent root expression
but are not the same stored closure or the same future matching state.

The compiler therefore composes the parents' retained contexts and Bakes that
context. It uses the RHS as a derivability check, not as a replacement for history.
All accepted concrete target states are compared to native saturation of their
actual parent contexts, including complete constructor rows and equivalence
partitions. This protects against losing facts behind a root-only equality check.

## Concrete result

The fixture starts with 13 concrete source instances. It Bakes 15 templates in
total and reserves 33 Cell rows for the initial state and possible outcomes.

| Stage | Compiled transition branches | Valid targets | Need alias Bake | No transition | Shared condition matrices |
| --- | ---: | ---: | ---: | ---: | ---: |
| First | 11 | 4 | 2 | 5 | 2 |
| Second | 9 | 4 | 0 | 5 | 1 |

The native match oracle confirms **6 + 4** full bindings, including the two
pending-specialization matches. Eight accepted target closures are exact.

Examples of continuation:

- Factoring produces a larger Cell, which participates in another factoring.
- Combining two powers produces a Cell that combines with a third power.
- An Integral/Cos transition exposes a Sin interface consumed by Diff/Sin.
- Diff/Sin exposes a Cos interface consumed by Integral/Cos.

The last two can create cycles in a saturated egraph. The offline representative
extractor uses grounded minimum terms rather than naive recursive traversal;
it does not confuse this with a cycle in the append-only execution schedule.

The larger factoring target represents **63 e-nodes / 23 e-classes**. The larger
power target represents **20 e-nodes / 13 e-classes**. These are native expanded
sizes, not GPU storage counts or evidence of a speedup.

Both FP32 and FP16-input/FP32-output GPU modes agree with the integer CPU
reference on every feature, condition, target coordinate, validity flag, and
transition status. The held-out data variation passes the same native and GPU
checks without recompiling the plan.

## Files and reproduction

- `rules.egg`: the selected real rule bodies.
- `compiler.py`: symbolic projection, retained-context composition, and wiring.
- `demo.py`: supplied schedule, independent native audit, and page generation.
- `test_program.py`: transition, effect-retention, alias, and continuation tests.
- `../moment_compiler/gpu.swift`: shared numerical backend and transition kernel.

```sh
python3 experiments/cell_spine/transition_program/demo.py \
  --output out/gpu-transition-next

# After native examples have been built:
python3 -m unittest experiments.cell_spine.transition_program.test_program -v
```

Use `--skip-build` to reuse the native binaries; use `--cpu-only` without Metal.
The output directory must be new. Full generated programs, native closures,
binary executables, and readbacks remain in untracked output directories. Only
the reproducible sources and compact `results.json` are committed.

## What is still missing

- Runtime discovery/routing of arbitrary external connections.
- A proof that an unrestricted rule set has a small transition/template family.
- Automatic target alias specialization and re-entry into the GPU program.
- Global e-class identities, hashcons, cross-instance union/rebuild, and target
  deduplication. Fresh reserved rows may denote equivalent Cells.
- Dynamic GPU allocation and compaction of successful outcomes.
- Efficient incremental feature maintenance on the GPU. Features are currently
  rebuilt for the reserved batch each stage, including inactive rows.
- End-to-end performance evidence. Small correctness timings include launch and
  audit-copy work; they are not a comparison with native egglog.

The precise new result is that **a successful numeric match can construct a
certified target Cell representation which another rule consumes on the GPU**.
It does not establish completeness of the offered connection set or solve
general-purpose dynamic egraph execution.
