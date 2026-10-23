# Parameterized transitions in executable spine-marker source

The old compact `manual_marker/long-after.egg` path already produced summary-to-
summary rules. The later typed transfer library preserved numeric slots and
native query wiring but did not lower their common spine into source markers.
This change connects the selected typed layout IR to executable `.egg` through
the existing `cell_spine.compiler` entry point, sharing `shared_prefix.describe`
and the existing source publisher analysis. It does not add a new solver or
infer a new closure proof.

Two checks use real native egglog:

* **If, two consumers:** their ten structural atoms become the same
  `CellSelectedSpine0`. The consumer needing `DUMMYCTX` reads it separately.
  Runtime indices `i,j` remain marker coordinates. No training indices become
  literals. Both rules keep their original effects. Input updates and union of
  the global context are included in the regression.
* **Numeric successor:** `State(x,n)` with `Port(x)` is shared by two rules.
  `advance` computes `n+1` once, constructs its State once, and directly publishes
  the successor marker using the already-known Port. `limit(x)` stays a current
  function lookup and the original guard remains. A regression removes repair
  after bootstrapping a controlled fragment: its second step still succeeds
  solely because the previous RHS published the successor marker.

Schematic generated rule (the actual file retains typed witness coordinates):

```egg
(rule ((StateSpine x n state port)
       (= len (limit x)) (< n len))
      ((Seen x n)
       (let next (+ n 1))
       (let state-next (State x next))
       (StateSpine x next state-next port)))
```

The generated code retains all original public facts. It is a query-reuse
backend, not the old forked-sort compact-storage backend. Initial inputs and
union-enabled combinations still require exact view completion before an
original consumer Run. This fallback is retained; this change does not prove
that all repair joins have disappeared or that Pass 3 is faster.

A private zero-prefix plan selects no materialized marker. The source backend
keeps that consumer and its runtime plan, rather than inventing a one-atom
receipt. `runtime-layout.json` contains only consumers not rewritten into source
markers. If none are rewritten, the output source is byte-identical to the input.

## Safety and evidence

The source backend materializes only immutable visible constructor/relation
facts. Function reads, global reads and numeric guards remain residuals. A
mutable or subsumable selected root retains its original consumer. Missing
payload coordinates use correlated receipts; independent projections are never
joined to invent a binding. Stronger existing closure programs are excluded
from publisher rewriting by exact rule identity.

RHS calls are put into let-bound form in their original order. A primitive or
mutable read is evaluated once; its actual value can feed a constructed row
and a successor marker. The publisher does not re-evaluate the formula after
another set. Only complete observations from the original LHS/RHS authorize a
publication. It does not assume a future If/DropAt closure has already run.

The existing global-environment validation/export from `1c8cdd1` is reused
(`parameterized_cell/environment.rs`, plus the native transfer exporter); no
training fact snapshot is inserted into the library.

`native-compare` results under `out/parameter-source-{counter,if}` compare all
original table rows, values, equality and visibility using the existing bench
skill; both are equal. `test_selected_source` additionally checks every explicit
finite-Run assertion, late function updates, union, shared If identity, and
one-time numeric evaluation. `generic_transfer` checks the selected runtime
library with the same referenced global inputs. These are correctness tests,
not timed Pass-3 results.

## Entry point

```sh
python -m tools.cell_spine.compiler \
  --source SOURCE.egg --selected-layouts boundary-programs.json \
  --source-markers --output NEW_DIRECTORY --skip-build
```

The file is the same native-checked layout IR used by the solver/runtime.
`source_sk_mk.egg` is executable source; `markers.json` records common prefixes,
residual atoms, publications and unconverted consumers. No solver runs during
this lowering. The fixed layouts in the small tests verify code generation;
they are not performance-selected plans. Production layout choice is being
moved to the per-workload/per-Run census in `planner-corpus` and
`--runtime-workload`, rather than initial table sizes.
