# Automatic Cell spine compilation

```sh
python3 -m venv out/cell-spine-venv
out/cell-spine-venv/bin/pip install -r tools/cell_spine/requirements.txt
out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/fixtures/assoc.egg \
  --output out/my-spine-run
```

The default `--mode auto` retains the existing parameterized constructor Cell
compiler when its contracts are sufficient. Other single-ruleset programs use
**concrete stage Bake**, as requested: compile the current input first, defer new
binding generalization. `--mode stages` selects that path explicitly;
`--mode symbolic` requires the original parameterized certificate.

Both paths build the native tools, use Gurobi, and validate on the actual local
egglog runtime. The stage path does not copy the source back as a pretend
compression. It records native checkpoint states and creates a shared, ordered
recipe DAG. `--analysis-dir DIR` applies to the symbolic path; stages need a fresh
native execution. `--skip-build` requires current native binaries. Output must be
empty.

## Concrete stage coverage

Supported and tested: relations, custom functions (`set`, arbitrary native merge
expressions, including non-idempotent merge and references to globals), primitive
conditions/calculations, finite `run`, `:until`, multiple execution boundaries,
late facts, `union`, `delete`, `subsume`, `push/pop`, `check/fail`, extraction,
named table printing, and includes. Container codecs preserve Vec order, Set
membership, Map key/value pairs, MultiSet multiplicity, Pair fields, and function
values including typed partial bindings. Multiple datatypes are supported.

Only observation/schedule boundaries and the final state are captured, rather
than one snapshot per seed insertion. A new compact representation retains all
original tables, function results, named roots, aliases, and row visibility at
those boundaries. It does **not** reinterpret finite execution as saturation.
Initializers are restored in declaration order, so globals used by merge
expressions are available before those functions are installed.

Stage outputs:

- `runtime.egg`: compact, shared **recipe spine**, with one `StageSummary` per
  checkpoint. This is an archive representation, not a generalized transition
  matcher. It does not materialize ordinary internal facts.
- `library.egg`: readable comments mapping recipe command IDs to native `.egg`
  declarations/actions. `stage-library.json` is the machine-readable codebook
  and exact prefix/middle/suffix recipes.
- `restore.egg`: expand the recorded states and perform checks/extractions in
  their original phases. Earlier phases use isolated scopes; the final native
  data remains available afterwards. It never runs original scheduled rules.
- `stage-bake/stage-NNNN.egg`: independently runnable full checkpoint.
- `stage-bake/stages.json`: original native witnesses, including all typed
  tables, globals and visibility. No raw rule-match trace is needed.
- `selection.lp`, `selection.sol`, `report.json`, `validation.json`: solver model,
  storage accounting, explicit contract and native differential checks.

Restore recipes describe immutable snapshots. Prefix/suffix sharing is **storage
reuse**, not instructions to apply a `set` twice to a previous mutable table.
Consequently a non-idempotent merge is not repeated. Statements retain their
order. Recipe reconstruction is checked byte-for-byte against the individually
validated native initializer streams. Helper reference constructors are
unextractable; they preserve cyclic classes without replacing the chosen IR by
an artificial cheap term. Extraction ties may choose another equivalent term
with the same cost. Auxiliary tables may appear in full table-size diagnostics;
original table contents and sizes are preserved.

The stage model chooses all earlier checkpoints as potential recipe parents,
using exact common prefixes and suffixes. Gurobi selects minimum weighted command
references and slice descriptors. There is no sampling, candidate budget or
heuristic solver fallback. The command dictionary cost is constant across these
choices. Search weight has no role in this mode: it is not a Join optimizer.
Archive bytes include the codebook and recipe `.egg`; no RSS/speed claim follows.
Some tiny programs are larger after encoding.

## Guarantees and remaining boundaries

`compiled_stages` means **the recorded concrete execution is recoverable**, not
that its snapshots are universally interchangeable saturated Cells. Changing the
source/input requires rebaking. Equal table states can have different semi-naive
frontiers: those frontiers are not restored, and this is not a native continuation
snapshot. New bindings, arbitrary future grafts and continued native execution
are not covered by this stage contract. Use the exported final data for inspection;
parameterized execution remains separate work.

The native program must finish on the supplied input. Nonterminating saturation
cannot produce a finite completed Bake. Proof generation, plugin-defined
commands/custom populated sorts without a codec, and external file writes are
rejected with an error, not silently copied or labeled compressed. Thus this is
broad standard single-ruleset support, not a claim about every possible egglog
extension. Header or codec errors also prevent publication.

The original symbolic path still requires complete positive constructor/equality
certificates and one unconditional saturation boundary. It emits typed fork
nodes, independent Summaries, observed boundary rules and cached-closure export.
Opaque-boundary equivalence is a positive substitution guarantee, not a promise
that unseen concrete binding/union interactions are already saturated. In auto
mode an unsupported symbolic lowering can use the concrete stage contract; the
reason is recorded. Solver/license errors remain errors. No engine changes are
introduced by either compiler.

## Multi-parent spine composition

`fixtures/dual_parents.egg` bakes three exact families. `A(x)` derives `AA(x)`;
`B(x)` derives `BB(x)`; both hidden facts enable `Joined(x)`. Automatic lowering
now produces this **actual rule**, rather than factoring just the first parent:

```egg
(rule ((Spine-Summary-0 parent_0_root_0 p0)
       (Spine-Summary-1 parent_1_root_0 p0))
      ((Spine-Summary-2 parent_0_root_0 parent_1_root_0 p0)
       (subsume (Spine-Summary-0 parent_0_root_0 p0))
       (subsume (Spine-Summary-1 parent_1_root_0 p0)))
      :ruleset Spine-Run :name "impl_2_0")
```

`fixtures/nested_parents.egg` additionally checks `A + B -> AB`, then
`AB + C -> ABC`: the combined spine is an ordinary parent for later baked
transitions, not a terminal archive.

Both parent spines remain shared. Their common `p0` is the binding constraint;
family identity alone cannot enable this transition. Neither `AA`, `BB` nor
`Joined` is materialized as a forked internal node. The larger Cell's Summary
refers to both existing roots; its cached closure is expanded only in
`Spine-Export`.

`fixtures/shared_family_graft.egg` demonstrates **two instances of one family**:

```egg
SummaryA(root0, x, y) + SummaryA(root1, y, z) + Spine-Gate(x, z)
    -> SummaryCombined(root0, root1, gate, x, y, z)
```

Instance identities and port mappings are distinct even though the two Summaries
use the same relation and Gurobi-selected representation. The external `Gate`
condition remains. A reversed Gate cannot trigger the composition. A new union
can enable a previously blocked binding, via ordinary egglog canonicalization
and incremental matching on the Summary constructors.

Only parents recorded in Bake history are eligible. Typed history-token mappings,
seed containment, and explicit equality prerequisites justify factoring. All
certified parents are retained together; unsupported portions remain in the
residual pattern. Equality obligations are never erased merely because both
endpoint nodes exist. Parents of the target family itself are left in residual
form to avoid self-dependent ingress. This is not arbitrary pair enumeration,
nor a general matcher for unbaked hidden-fact interactions.

## Binary optimization

For each exact Cell family `f`, choose one observed representation `p`:

```
x[p] in {0,1}; sum(p in family f) x[p] = 1
```

A transition is a hyperedge with all its parent instances, their typed binding
maps, residual facts/equalities, and a target. First compile every endpoint
representation tuple on this observed hyperedge. Then give each implementation
`o` a binary `z[o]`:

```
sum(o implements observed hyperedge e) z[o] = 1
z[o] <= x[p]   for EVERY parent and target representation required by o
```

Two instances of the same family retain separate bindings but share the one
representation choice. A tuple with no legal implementation cannot be selected.
This couples both parents and the target, rather than pricing two unrelated
pairwise edges. The selected implementation is the input to the certified
ancestor-takeover pass described below; its final rules are reported separately.

For each alpha-local template shape `n`, `y[n]` is the OR of all selected
representations using it. Objective:

```
storage_weight * sum(y[n])
+ search_weight * sum(distinct_lookup_terms_and_guards[o] * z[o])
+ update_weight * sum(distinct_write_terms_and_unions[o] * z[o])
```

The native expression parser counts **nested** constructor calls, Summary calls
and explicit equalities/unions from the actual generated rules. Lookup/write
counts are structural proxies, not measured Join cardinalities or timings.
Template sharing refers to code shapes, not automatic identity of runtime
instances. Reusing parent root variables avoids rebuilding those trees in RHS.
Each observed path contributes once to transition costs; duplicate emitted
rules can share one declaration. Workload frequencies are not extrapolated.

Weights default to 1 and are explicit CLI arguments. There is no candidate
budget, sampling or greedy fallback. No search over arbitrary parent subsets is
introduced: all certified parents of the observed composition are used. For
hyperedge `e` touching distinct endpoint families `F_e`, the number of compiled
options is `product(f in F_e) |P_f|`; total options sum over observed hyperedges.
This can still grow combinatorially with representation alternatives. Gurobi's
optimality is only for this finite option set and proxy cost. Solver/license
errors are surfaced, not worked around by quietly dropping options.

`selection.lp`/`.sol` expose the actual coupled model. `transitions.json` records
all options, port maps, required representations, query/write terms and costs;
`report.json` lists selected implementations and the final emitted rules.
`retirement.costed_transitions` retains the pre-takeover implementations. Concrete stage
recipe selection remains a separate, per-stage problem. `StageConcat` is still
archive sharing and is not used as a substitute for these Cell transitions.

Reproduce the native two-parent and same-family cases with:

```sh
out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/fixtures/dual_parents.egg \
  --mode symbolic --output out/spine-dual
out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/fixtures/shared_family_graft.egg \
  --mode symbolic --output out/spine-shared-family-new
```

## Retiring a parent Summary without losing its consumers

Summaries are now constructors returning `Spine-Cell`, so egglog can subsume a
**specific Summary row**. The multi-node spine remains shared. No fresh instance
counter, duplicated small-Summary view, engine patch, or union of different Cell
closures is introduced.

From the typed construction witnesses, the compiler builds a projection from a
larger canonical Summary to each smaller parent's roots and parameters. It
composes these projections transitively. Constructor expressions needed by a
projection must resolve in the owner's certified saturated snapshot; the owner
emits any required support nodes/equalities when it is created. This avoids
assuming that a semantically known hidden row is physically queryable.

Each baked consumer of a small Summary receives versions that read the appropriate
larger Summary directly. Multiple operands can be covered independently, including
two instances of one family. External facts and equality guards remain in the
body. There is no intermediate relation that reactivates the small Summary.

A composition's RHS now publishes its canonical result and subsumes the directly
matched, certified parent Summary rows. It does not subsume a larger owner merely
because that owner supplied a smaller view. Additional retirement rules handle a
small Summary arriving after its covering owner. Under normal `saturate Spine-Run`,
all such covered rows are inactive at the fixed point. Rules may still match in
the same execution batch before retirement commits; there is no claim of zero
redundant matches or ordering between rules within a batch.

Inheritance covers every compiled Summary consumer. Raw coarse-input queries
still exist, and shared Add/Mul rows remain visible. Re-inserting a retired
Summary does not reactivate it. Inherited matchers remain available after new
external facts or union/rebuild, as verified by the late-input tests. Closure
export uses active owners and reconstructs the complete original constructor
graph, including the facts of retired parents.

A finite congruence check removes inherited rules only if their target is already
covered **and** all their writes/equalities are already guaranteed materialized.
Constructor injectivity is never assumed. Alpha-equivalent declarations are
shared. Cyclic family-dominance edges are recorded but do not authorize retirement:
mutual disappearance is not a proof. No budget or sampled candidate cutoff is used.

The current implementation expands the alternatives for each consumer operand.
With `k_i` owner projections for operand `i`, it considers `product_i(1+k_i)`
forms before proven-redundancy removal. This can enlarge rule code and Join work.
The Gurobi objective still prices pre-inheritance plans; it does not optimize
this expansion. Reported objective optimality must not be read as optimality of
the final runtime code. `retirement` exposes projections, retained cycles,
emitted consumers, retirement rules and the count of proven-redundant forms.

Native regressions cover: Q arriving after Small is retired into Large; a later
union enabling Q; two retired instances joining on a late Gate; an uncovered
instance remaining active; transitive retirement; and an ablation that removes
inherited rules and must fail. Since native `check` deliberately includes
subsumed rows, tests also use late-installed real matchers and inspect active row
counts. Total and active Summary counts are reported separately.

```sh
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_takeover -v
```

## Verification

```sh
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_spine -v
```

For both paths:

```sh
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_spine tools.cell_spine.test_stages -v
```

Tests include global sharing versus independent minima, exhaustive verification
of a three-endpoint hyperedge model, incompatible endpoint tuples, two-parent
composition, same-family instances, wrong/missing external facts, union-enabled
transitions, preserved equality guards, actual native analysis on multiple programs, ordinary
row isolation, full cached-export equivalence, delayed boundary grafting,
non-injective binding, and negative tests for wrong union, ordinary-node leakage
and secretly running original rules. Relation/function stage compilation is executed on native egglog, together with
upstream list and merge programs. Mutated function values and lost subsumption
are rejected by the independent witness checker:

```sh
target/release/examples/spine_stages --verify out/my-spine-run/stage-bake/stages.json
``` These are correctness checks, not a speed/RSS benchmark.
