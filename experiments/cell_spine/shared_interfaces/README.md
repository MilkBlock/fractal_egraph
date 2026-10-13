# Effect elimination and shared seed interfaces

This modifies the production Python Bake compiler and emits legal `.egg`
rules. Measurements execute those rules in the actual local egglog runtime;
this is not a standalone simulation and does not modify the kernel. `Add2` and
`Add4` describe the fixture, not special cases in the implementation.

## Changes

1. `takeover.entailed` uses installed spine/support equalities. It processes RHS
   actions in order, so an earlier retained effect can imply a later one. It
   never infers constructor injectivity or materialization from hidden closure
   membership. The fixture's emitted write-cost units fall from 503 to 443.
2. Selected consumers can share a seed decoder. For the selected four-leaf
   spine, the generated maintenance rule is:

   ```egg
   (rule ((Summary-1! r0)
          (= ab (Add!! a b)) (= cd (Add!! c d)) (= r0 (Add!! ab cd)))
         ((Interface-1! r0 a b c d ab cd))
         :ruleset Run! :name "interface_1")
   ```

   Consumers read both `Summary-1!` and `Interface-1!`. The latter alone is never
   permission to execute. Retirement stops consumers even if cached rows remain.
   Native rebuild/union may enable new correlated tuples; maintenance is in the
   same incremental ruleset and retains all decoder guards. Intermediate values
   let RHS construction reuse subtrees without another decoder query.
3. The solver jointly chooses spine, marker anchor/payload/storage and inline
   versus shared decoding. It pays for the interface schema and maintenance
   once per family, and for each consumer read. Native instance-storage mode
   additionally counts actual retained view rows, including stale rows. Every
   emitted rule's structural price is checked by the native parser.

`--cell-interfaces auto` is the default. `inline` is the no-view ablation;
`shared` restricts layouts with decoders to use the view. Layouts without a
decoder remain direct. No budget, heuristic pruning, or Add-specific rule is
introduced. The model is still structural, not a measured-frequency/time model.

## Single-run comparison, 2026-10-14

Fixture: `../fixtures/add2_pair.egg`, 128 independent pairs. Native small graphs
are saturated before timing; compact inputs already contain small canonical
Cells. **Run** includes the new connecting fact plus saturation. Interface
maintenance is included. Counts/probes use separate diagnostic processes.
Each variant was timed once; no average, minimum selection or speed claim for
unseen programs. The native comparator uses the same local kernel, not pristine
upstream. Source/seed validation compares full closures; replicated benchmark
verification compares expanded node/class counts.

| Variant | Run ms | Setup + input + Run ms | Candidate row visits | Physical rows | Rules |
|---|---:|---:|---:|---:|---:|
| Native | 7.926 | 44.132 | 13,568 | 4,224 | 2 |
| Previous compiler (`da6e8ed`) | 14.462 | 65.792 | 59,904 | 2,560 | 62 |
| Stronger effect elimination, inline reads | 13.004 | 61.345 | 59,776 | 2,560 | 62 |
| Jointly chosen shared interface | 9.552 | 52.936 | 36,352 | 2,816 | 63 |

All four restore **3,328 e-nodes / 1,408 e-classes**. The compact variants end
with 128 active and 384 retired markers. The selected layout shares the larger
Cell decoder (7 columns) and keeps the smaller Cell inline. The 256 extra rows
are interface storage: this optimization is **not** a memory reduction over
the previous compact program. Candidate row visits count repeated work across
join stages, not distinct rejected matches. Rule matches are logical matches,
not committed mutations.

The shared version reduces Run by about 34% against the previous compiler in
this sample, but remains about 21% slower than native. Schema-level lookup/guard
units drop from 488 to 317; those units are not runtime row counts. Reusing the
same capture, inline Bake took about 3.21 s and auto Bake about 5.96 s (see JSON
for exact phase timings); adding choices is not a Bake speed optimization.

**Remaining limitation:** the 62 inherited transition/retirement rules still
exist. Sharing their seed decoders does not yet combine the Cartesian product
of owner alternatives into one consumer. The extra rule is the shared decoder.
This change reduces repeated queries while preserving their existing contracts.

## Reproduce

```sh
EGG_LAYOUT_CELL_WINDOWS=1 out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/fixtures/add2_pair.egg \
  --output out/shared-interface-inline --cell-interfaces inline --skip-build
out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/fixtures/add2_pair.egg \
  --analysis-dir out/shared-interface-inline/analysis \
  --output out/shared-interface-auto --cell-interfaces auto --skip-build
out/cell-spine-venv/bin/python experiments/cell_spine/seed_composition/late_bench.py \
  --variant effects=out/shared-interface-inline \
  --variant interfaces=out/shared-interface-auto \
  --copies 128 --output out/shared-interface-comparison
```

The archived `before` variant in `results.json` uses the previously generated
library from `da6e8ed`; hashes are included. It is not recreated by the current
inline mode, which keeps stronger effect elimination. There is no build time
in the runtime table.

Tests cover non-Add interface generation, brute-force agreement with the MIP's
shared/inline choice, maintenance charged once, no unused-interface selection,
late union, guards, argument correlations, retired owner exclusion, and the
existing full native Cell/export/late-composition regressions. CI includes them.
