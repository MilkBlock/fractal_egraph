# Stage-gated CPU patches

For a saved native staged plan, run the native CPU driver from the repository root.

```sh
cargo cpu --source input.egg --output out/staged --mode staged --plan PROGRAM.json --stage-audit
```

`--stage-audit` is optional and records stage locations, table sizes, ordered
updates, local merge expressions and actual replies. It is diagnostic overhead.

## What executes

The compiler reuses symbolic composition from the analyzer to prepare RHS
recipes, including across finite repeat boundaries. This is **preparation**, not
permission to turn a finite repeat into saturation. The source schedule remains
the only execution schedule.

```
native match in Stage A
    → instantiate one correlated local frame
    → prepare A/B/C constructor and arithmetic expressions with local references
    → publish only A's RHS through native egglog
    → native merge, union and rebuild

native match in Stage B
    → find the prepared patch using its complete table-binding key
    → supply the actual guard results and current function ports
    → publish only B's RHS
```

Every publication still has a complete native LHS match. This version therefore
does **not** eliminate the original Free Join, prove a future match merely from a
template, or claim Cell sat-skip. It proves the CPU preparation/publication
protocol with exact native matching as its activation authority. The prior eager
target remains available as `--execution eager` and keeps its stricter admission
checks; the two execution modes are explicit.

## The three mechanisms

1. **Local references.** A frame stores typed `Value`, `Construct`, `Compute`,
   `Read` and unresolved slots. `TCPair(@term,cost)` can refer to an unallocated
   `TermBop` slot. Preparing the DAG does not insert a row or allocate an eclass.
   Only the current matched RHS materializes its required nodes. Native hashcons
   resolves duplicate constructors across frames. Exact integer primitives use
   the native implementations, including their failure behavior.
2. **Merge dependencies.** Each active Run has an ordered update log. Repeated
   writes to one key compose a local merge expression; for example:
   `q1=Smaller(p10,p7); q2=Smaller(q1,p5)`. Pure arithmetic/min/max can be evaluated
   in this local representation. All original set requests are still submitted
   to the native merger, which remains authoritative. This implementation does
   not silently replace custom callbacks or collapse their observable nodes.
   At the boundary, the log is reconciled with actual canonical function values;
   an unresolved/different prediction does not authorize an effect.
3. **Stage visibility.** Each executed Run has its source path, monotonically
   increasing occurrence number, and complete repeat/saturation iteration stack.
   A future patch remains local until its rule is matched in an actual Run.
   Multiple rules in one Run do not consume newly prepared facts as if another
   Run had happened. Native Run-until checks retain their entry-state meaning.

## Function ports must be versioned separately

Suppose B reads `Best(x)=7`, then an intermediate stage changes it to 1, and C
reads `Best(x)` again. B's derived node must keep its bound 7; C must receive 1.
The two steps therefore have distinct read-port nodes even when they refer to
the same function key. B consumes/fixes its own binding, not C's future port.

The readiness pass canonicalizes identities and rereads keyed ports from the
current native state. A custom merge result is represented by its **actual
function reply**, never by the last proposed candidate. Key unions can introduce
new merge-created nodes. Native rebuild and residual queries discover these;
the next readiness pass sees the updated keys and replies.

This is not yet an incremental subscription index: readiness currently revisits
live frames. Nor does the local merge prediction replace global conflict
resolution. Both limitations are visible in the first timings.

## Lifetime and safety

Frames belong to actual seed matches, not the Cartesian product of all rows and
all plans. Completed frames are released. A ready patch not accepted by the
scheduled native query is retired with its speculative descendants. Unready
ports can wait; source commands outside a Run conservatively invalidate pending
frames, and program completion clears them. No candidate cap or retry budget is
used to drop inference: native queries remain present throughout.

Supported RHS operations include constructor/read/copy, selected pure integer
and Boolean primitives, set and union. Unsupported/opaque operations continue
through the native source rule. The CLI uses one native thread; parallel callback
ordering is not certified by this prototype.

On successful runs, tests compare full typed facts, function values, equality
partitions and subsumption against native execution. A failed future arithmetic
expression does not fail the program before its Stage is actually activated.
Error tests currently check native/staged rejection, not equality of every
partial table after an error.

## Source and reproducible checks

- Native runtime: `egglog/src/cpu_cells/staged.rs`.
The following preparation and experiment tools are available in the [research snapshot](https://github.com/MilkBlock/fractal_egraph/tree/bf5b4539d09ad96473e40875868a8f4f02d6add5).

- CPU preparation adapter: `eggfuse/compiler/targets/cpu.py`. The shared GPU
  analyzer is unchanged; its composition results are adapted to actual function
  return ports, without exporting a synthetic saturation as source control.
- `python3 experiments/cpu_multistage/staged_check.py --output out/staged-check`
  covers stage isolation, same-ruleset next-Run boundaries, finite repeat,
  function replies, aliasing, duplicate constructors, custom merge chains,
  late ports, errors, fresh-process replay and new facts with a saved plan.
- `python3 experiments/cpu_multistage/staged_bench.py --output out/staged-bench`
  runs the three small frozen Pass 3 inputs once and reuses previously verified
  native controls. `--controls DIR` selects the saved control suite explicitly.

The retained control times are labeled historical. Do not attribute any timing
variation to Cell inference when the full native query is still being paid.
