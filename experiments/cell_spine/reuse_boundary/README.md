# General Cell reuse boundary (native runtime)

This is a diagnosis of the existing history/Gurobi compiler. There is no
operator-specific compiler or multiset/partition lowering. Bake the existing
three-leaf `assoc.egg` fixture once, freeze its two-family library, then execute
six new trees using that library. The real native validator compares complete
constructor tables and equality partitions after export.

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/reuse_boundary/probe.py \
  --output out/reuse-boundary-new --skip-build
```

| Input | Native constructor rows | Restored rows | Result |
|---|---:|---:|---|
| 3 leaves, left-associated | 15 | 15 | equivalent |
| 3 leaves, right-associated | 15 | 7 | rejected |
| 4 leaves, left-associated | 54 | 26 | rejected |
| 4 leaves, right-associated | 54 | 10 | rejected |
| 7 leaves, left-associated | 1,939 | 59 | rejected |
| 7 leaves, right-associated | 1,939 | 19 | rejected |

`report.json` is the concise result. Full manifests and native state differences
are reproducibly generated in the requested output directory, not checked in.
These are failed generalization cases, **not successful Cell compilation** or
performance results. This does not establish that retraining on the seven-leaf
input cannot work; it establishes that this frozen small library is insufficient.

## Where the missing reuse occurs

The pair Cell's cached closure contains both `Plus(a,b)` and `Plus(b,a)`.
Its selected physical spine and ingress only match the materialized shape.
`closure_export` emits the alternate row under `Spine-Export`, after compressed
inference has already finished. A right-associated tree therefore creates pair
Summaries but no triple Summary: the larger consumer requires a left child
`Spine-Plus` that is known in the closure but absent from the physical spine.

`parent_wirings` factors recorded parent instances. `visible_projections` can
replace a hidden row when its result is already a literal, input port or visible
root. It does not supply arbitrary internal closure values as future binding
interfaces. `takeover_lenses` preserves already compiled consumers after
retirement; it does not synthesize the missing closure-row queries.

Even the left-associated seven-leaf input activates triple Summaries and still
restores only 59 rows. Adding one missing orientation alone is insufficient:
internal rows produced by composed closures also need to participate in later
matches. The general missing component is a certified closure-query/binding
interface for successor composition. Gurobi cannot select implementations that
were never generated.

The next repair belongs in that general interface/transition generation, with
native differential validation on these cases. Recognizing an AC law by name
or shape and injecting a dedicated closure algorithm would bypass the issue.
