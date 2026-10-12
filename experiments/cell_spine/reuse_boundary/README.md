# General Cell closure interfaces (native runtime)

Bake `../fixtures/assoc.egg` once using the existing history/RipSatCell/Gurobi
pipeline. Freeze that three-leaf, two-family library and run six new inputs.
No operator-specific compiler, law recognizer or multiset lowering is used.

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/reuse_boundary/probe.py \
  --output out/reuse-boundary-new --skip-build
```

Use current native `egg_layout`, `lhs_ast` and `spine_validate` binaries, or omit
`--skip-build`. This is an explicit saturation test. It does **not** reinterpret
the user's finite `(run add-ac 7)` as saturation.

| Input | Old restored / expected rows | New restored rows / classes | Physical spine | Total native rows before export |
|---|---:|---:|---:|---:|
| 3 leaves, left | 15 / 15 | 15 / 7 | 7 | 46 |
| 3 leaves, right | 7 / 15 | 15 / 7 | 7 | 46 |
| 4 leaves, left | 26 / 54 | 54 / 15 | 17 | 223 |
| 4 leaves, right | 10 / 54 | 54 / 15 | 17 | 223 |
| 7 leaves, left | 59 / 1,939 | 1,939 / 127 | 251 | 16,246 |
| 7 leaves, right | 19 / 1,939 | 1,939 / 127 | 251 | 16,246 |

`before-frontier.json` preserves the negative baseline (commit ab9ed1d).
`report.json` records the corrected runtime. The native validator checks complete
constructor rows **and** equality partitions, not only these counts. No original
rules run during spine inference or export. `Spine-Export` expands the finite
cached Cell closures only after inference. Each output directory includes actual
`*-runtime.egg`, `*-restore.egg`, native manifests and detailed validation results.

## What changed

Previously the pair Cell knew both `Plus(a,b)` and `Plus(b,a)`, but its alternate
row was visible only at final export. A successor needed a physical constructor;
internal values that were neither input slots nor selected roots could not become
new bindings. Thus the same small library could not close even a right-associated
three-leaf input.

The compiler now emits typed `Spine-Read-Plus(left,right,result)` interfaces from
certified Cell closures. Successors read these interfaces, including internal
values, while retaining their Summary, equality and external-fact guards.
Constructor witnesses give new internal values identities; read congruence
unifies results of the same constructor on canonical arguments across Cells.

RHS construction is phased: first propagate interfaces and requests, then retire
requests already answered by known values, then create only missing witnesses.
This avoids rematerializing every known constructor when a consumer needs its
result. The generated schedule must be preserved. Retired smaller Summaries do
not become active again; certified larger owners inherit their consumers and
interface publishers.

## The remaining storage problem is explicit

The repaired library is **53 lines / 8,479 bytes**, shared unchanged by all six
inputs. But a short library and fewer physical constructors do not imply a small
runtime graph. On seven leaves:

| Native table | Rows before export |
|---|---:|
| Spine-Leaf | 7 |
| Spine-Plus | 244 |
| Spine-Read-Plus | 1,932 |
| Spine-Wanted-Plus | 1,925 |
| Spine-Summary-0 | 1,932 |
| Spine-Summary-1 | 10,206 |
| **Total** | **16,246** |

Counts include subsumed requests and Summaries. Original native storage has 1,939
rows. Rows have different widths; neither count is RSS. There is **no claim of net
compression or speedup**. Flat interfaces still enumerate all 1,932 Plus rows;
Summary identities still enumerate different bindings of the same small Cell.
Compact binding/summary sharing remains unresolved. Gurobi selects the original
spine implementations; the added interface support is outside that objective.

`test_frontier.py` locks in the six full-state comparisons, late union/congruence,
primitive guards and reuse without redundant physical construction. Its ablation
removes certified interface publication and verifies that the right-associated
case fails whole-state comparison again. Existing retirement, multiple-sort,
noninjective-binding and staged-archive tests remain enabled.
