# Multi-node Cell spine

A standalone generated-rule prototype, executed by the actual local egglog
runtime. It does not yet modify the analyze/Bake pipeline or claim a speedup.

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
