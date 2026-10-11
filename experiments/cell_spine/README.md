# Multi-node Cell spine

The general automatic Bake/Gurobi compiler is now in
[`tools/cell_spine`](../../tools/cell_spine/README.md). The files in this directory
retain the original small Add demonstration and provide its regression fixtures.

The original `demo.egg` is a standalone generated-rule prototype executed by
the actual local egglog runtime. `automatic.egg` is produced by the new native
Bake/Gurobi pipeline from `fixtures/assoc.egg`; `automatic-results.json` records
five automatically compiled programs. Neither is a speed/RSS benchmark.

```sh
python3 experiments/cell_spine/generate.py
cargo run --offline --release --example cell_spine_verify
python3 -m unittest discover -s experiments/cell_spine -p 'test_*.py'
```

`demo.egg` retains the original Math constructors and AC rules. The selected
left-associated source structure is represented by multiple `Spine-Add` nodes,
with `Input` references to actual Math values. `Summary-Add3/4` are independent
relations on the spine root; they do not enter structural constructor keys.
The shared fork type intentionally lets Add3 and Add4 share the same prefix.

The explicit `Extend` boundary fact and a summary enable one specialized
transition, which retains the old spine and appends one node. No internal AC
closure is materialized. `Export` recursively reconstructs the selected ordinary
expression on demand, after which the unchanged original rules apply.
The Summary interpretation is AC equivalence of this positive Add family; no
general claim about hidden facts, arbitrary rule environments or function updates
is made. Reified is deterministic for this immutable structure, so its merge
keeps the existing result.

Native verification checks ordinary Add isolation, shared-prefix row counts,
repeated-graft deduplication, and exported expression equivalence under the
original rules. These are actual table observations, not mutation trace counts.

Limits: explicit ingress rather than automatic history recognition; finite
whitelisted sizes; append-only boundary transitions; no automatic original-row
subsume, arbitrary internal touch queries, or general closure certificates.
Original shared rows cannot safely be subsumed without an ownership/visibility
contract. Unsupported grafts remain unsupported rather than silently expanding
the whitelist. Input Math references participate in native canonicalization;
the summary does not assert that arbitrary unions preserve its baked interface.
The 0-1 path-selection optimization is deliberately not implemented here.

Current-input stage compilation is covered by `fixtures/stages/` and
`stage-results.json`: 14 programs / 55 native checkpoints, including upstream
list and merge examples. The stage archive is intentionally distinct from the
parameterized Add spine above; its full contract is in the compiler README.

## Automatic multi-parent composition

The symbolic compiler now uses every certified parent of an observed composition.
See `fixtures/dual_parents.egg`, `fixtures/shared_family_graft.egg` and
`fixtures/nested_parents.egg`. Generated runnable exports are
`dual-parent-generated.egg`, `shared-parent-generated.egg`, and
`nested-parent-generated.egg`. `hyperedge-results.json` records the selected
Gurobi hyperedge implementations and native whole-state comparisons. These are
actual local egglog runs, not a simulated archive composition or a performance
benchmark. The original rules remain declared but never run on the compressed
path. `AA`/`BB`/`Joined` smooth interiors appear only on explicit export.

The regression suite also delays the second parent / Gate, reverses the Gate
binding, and unions previously different input ports. It verifies that only the
correctly wired summaries can trigger, including two instances of one family.
The larger result can participate in a later two-parent transition. A tiny
three-endpoint ILP is checked against exhaustive enumeration.

Run `python -m unittest tools.cell_spine.test_spine tools.cell_spine.test_stages -v`
using the environment containing Gurobi. Full compiler commands and the exact
cost model are in `tools/cell_spine/README.md`.
