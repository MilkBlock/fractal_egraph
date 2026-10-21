# Publish correlated spine witnesses in generated egglog

The existing Bake compiler now offers `--cell-interfaces publisher`. It adds
a direct-publication ABI to the joint spine/layout solver: the rule that creates
the canonical seed also stores intermediate identities in its Summary. No
separate marker-table scan reconstructs them. This changes generated `.egg`,
not the egglog engine. It is separate from the opt-in CPU/GPU receipt bridge in
`experiments/commit_spine_markers`.

For a seed `B(A(x), y)`, these are the three implementations in the controlled
lowering test:

```egg
; Inline: each consumer decodes the seed.
(rule ((Summary-0! root) (= ax (A!! x)) (= root (B!! ax y)))
      ((Out0! ax y)) :ruleset Run!)

; Shared decoder: consumers read a view maintained by an additional rule.
(rule ((Summary-0! root) (Interface-0! root x y ax))
      ((Out0! ax y)) :ruleset Run!)

; Published: the existing producer stores root, x, y, ax together.
(rule ((Summary-0! root x y ax))
      ((Out0! ax y)) :ruleset Run!)
```

These are alternatives with different Summary schemas, not declarations to
combine into one program. `run.py` emits complete executable `.egg` files for
each. The direct marker stores one correlated tuple; it does not combine the
`x` from one application with the `y` from another. It is an active owner marker,
so retiring it disables its consumers. Native rebuild updates stored identities.
Explicit seed equality guards remain. No constructor injectivity is assumed.

## Native lowering experiment

128 independent instances, 12 consumers using the same internal subtree; one
execution per variant. Producer work and shared-view maintenance are included.
The solver chooses among the three ABIs using measured native search work plus
the existing column/update cost. This fixture controls one seed/layout domain;
it is **not** a fresh Analyze run or a claim about eggcc coverage.

| Representation | Index handles | Index-returned row visits | Logical applications |
|---|---:|---:|---:|
| Inline decoding | 4,762 | 8,064 | 1,664 |
| Shared decoding | 668 | 4,224 | 1,792 |
| Direct publication (selected) | 130 | 384 | 1,664 |

All three have exactly **2,304 public constructor rows / 2,048 e-classes**,
with complete typed graph/equality comparison. Auxiliary Summary/view schemas
are excluded from semantic equality, not from execution. These counters do not
include every primary cover scan; 384 is not the number of all rows touched.
Logical applications are not committed mutations. Recorded submillisecond
instrumented timings are not a speedup claim. The comparator is the current
native kernel, not pristine upstream or the GPU runtime.

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/published_markers/run.py \
  --output out/published-marker-abi
```

`results.json` records the single native run, binary/source-program fingerprints,
full graph comparison, and solver choice. Generated complete examples are named
`m0_0.egg`, `m0_0_shared.egg`, and `m0_0_published.egg`.

## Full compiler verification

`pipeline.egg` is one genuine distributivity rule. It went through fresh native
Analyze, certified Cell extraction, joint optimization including measured
search, `.egg` generation, and complete native restoration/equality checking.
Both source and symbolic seed checks passed: **8 nodes / 7 e-classes** each.
The solver selected a cheaper existing borrowed/narrow representation here;
having a publisher option does not force it to be used.

```sh
out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/published_markers/pipeline.egg \
  --output out/published-marker-pipeline \
  --cell-interfaces publisher --search-cost native --skip-build
```

The existing long-spine graft fixture also passed all three native checks with
structural pricing and the expanded layout domain. A native-cost search for that
larger domain was stopped during evaluation and produced no optimum or published
executable. Native pricing is an offline experimental option: exact conditional
enumeration can be expensive even when executing one candidate is fast.

## Boundaries and tests

This extends the existing positive-constructor, saturated, baked-transition
backend. It does not newly support arbitrary finite Run schedules, set/merge,
delete, unknown grafts or every binding enabled by arbitrary outside union.
Keeping alternatives in the solver is not a runtime completeness fallback.
No claim is made that union can never create an unseen binding; the existing
whitelist and native validation contract remains the boundary.

Tests cover direct birth, shared consumers, intermediate reuse, root/input
unions, correlation, explicit guards, retired-owner exclusion, failed-search
cost with zero matches, combined storage/search cuts, measurement failures,
and fresh Analyze-to-native validation. Existing layout/pricing, borrowed
storage, interface and decoder regressions also pass.

```sh
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_publisher -v
```
