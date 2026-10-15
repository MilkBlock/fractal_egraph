# Automatic Cell spine compilation

```sh
python3 -m venv out/cell-spine-venv
out/cell-spine-venv/bin/pip install -r tools/cell_spine/requirements.txt
out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/fixtures/assoc.egg \
  --output out/my-spine-run
```

The default `--mode auto` and `--mode symbolic` both require a successful
compilation through the existing general Cell pipeline. Unsupported inputs and
failed validation are errors. **There is no automatic snapshot fallback and no
operator-specific AC compiler.**

Use `--mode stages` only to explicitly export a debug snapshot archive. This is
not Cell compilation. It preserves current-input checkpoints, not reusable
coarse paths. Its output filenames and status are distinct from Cell results.
`--analysis-dir DIR` applies only to Cell compilation. `--skip-build` requires
current native binaries. Output must be empty.

## Cross-fact project knowledge

Pass `--knowledge-library .egg-fractal/knowledge.json` to reuse and update a
project's validated Cell contracts. The VS Code **Bake Active .egg** command
passes this automatically for the workspace (or source directory without a
workspace). CLI runs without the flag remain isolated and reproducible.

The library partitions native-parser-normalized declarations, ordered rules and
schedules independently of input facts/checks. Changes to these environment
commands create a separate namespace. This conservative key preserves names and
order; it does not prove two differently written rulesets equivalent.

Successful Bakes persist shared saturated graphs, portable seed recipes and
explicit parent binding substitutions. Imported contracts join the actual
spine/interface optimization, and EVERY imported seed is checked again by the
native validator before successful output/publication. Different runs' event
IDs are never used to connect parent bindings. Source edits during Bake reject
publication. Atomic replacement under a POSIX file lock preserves concurrent
updates. The current constructor/equality backend restrictions still apply.

Merge-family learning aligns whole typed source/target/obligation descriptions,
including shared closure-state identities. Repeated differences share one hole
across the entire description, preserving alias correlations. Typed structural
anti-unification proposes common contexts from observed descriptions; no Add,
Tuple, or operator-specific family is hardcoded. A MIP selects a dictionary and
residual bindings minimizing structural description size. Stored transitions
use that dictionary; loading checks exact reconstruction hashes. Closures are
stored once per exact native equivalence class, independently of seed recipes.

This is finite evidence compression, **not arbitrary-size recursive induction**.
`observed` templates describe exact admitted transitions;
`generalized-hypothesis` templates summarize multiple descriptions but do not
authorize unseen transitions. Historical exact Cells can affect generated code;
hypotheses do not bypass the matcher or publish unproved unions. Tier0 capture
still runs on each new input. No tier0 acceleration claim follows from a smaller
description or a larger sample-support count.

Each successful run writes `merge-families.json` and a readable
`merge-families.md`. To list CURRENT source evidence without Bake/analyze:

```sh
out/cell-spine-venv/bin/python -m tools.cell_spine.knowledge list \
  --source input.egg --library .egg-fractal/knowledge.json
```

VS Code command: **Egg Fractal Analyzer: List Merge Families for Active .egg**.
It refuses unsaved/stale input and shows evidence, parameter types and support
from other inputs. Repeating a fact program does not add another sample. The
counts describe admitted transition descriptions (including certified seed
cuts), not runtime matches or committed mutations. Read-only listing does not
create a missing library.

Current limitations: all historical contracts in a compatible namespace are
admitted to optimization; the added layouts can increase Bake time. Pairwise
anti-unification is restricted by typed merge envelopes but has no candidate
budget; large corpora can be expensive. Only selected previous templates are
retained as additional proposals. The MIP is optimal over those proposals,
not over all recursive programs, and its structural cost is not a byte/time
model. See [the native three-input example](../../experiments/cell_spine/knowledge/README.md).

## Alternative entries from LHS coverage

For the project-level method (history, ripen, closure identity, spine and boundary
transitions), start with the [main README](../../README.md). This document details
the compiler's supported contracts and optimization model.

`--lhs-coverage report` indexes complete and anchored partial LHS matches in known
closed Cells. The table preserves typed bindings, shared variables, remaining
conditions and seed equality obligations. `--lhs-coverage entries` additionally
emits one-rule alternative entry contracts; the default is `off`.

For a certified seed K with Sat(K)=D, form E from a matched LHS plus seed facts
not supplied by that rule's RHS. Admission checks that E and the resulting
assertions stay inside D, all input ports remain bound and equality obligations
are retained. E then supplies K after one application. Every emitted entrance
also passes native full-state comparison before publication. These are inferred
entrances, not invented history events or new closure families.

The output `lhs-coverage.json` records the index and evidence. Generated runtime
rules are in `library.egg`. No recursive predecessor search is performed, and
primitive/destructive environments or automatic aliased-port transport remain
outside this proof. See [the fixed-library held-out comparisons](../../experiments/cell_spine/lhs_coverage/README.md).

## Shared seed interfaces

In `--cell-composition seed-cuts`, known target seeds can additionally use
`certified_boundary_cut` interfaces. These expose alternative port-visible rows
of a parent's certified closure, including orientations absent from its stored
canonical spine. The target must contain all parent seed facts/guards in its
closure, under the complete typed binding; source environments must match and
be positive constructor/equality ASTs. Existing multi-parent bindings are not
expanded into redundant parent-subset consumers. This also repairs a real
same-family left/right graft omission. See [the graft audit](../../experiments/cell_spine/graft_diamond/README.md).

`--cell-interfaces auto` (default) jointly chooses inline seed decoding or a
shared `Interface-N!` relation for each selected marker layout. `inline` keeps
decoding inside each consumer; `shared` restricts layouts with a decoder to the
shared form, for ablation. Layouts needing no decoder remain direct.

A view stores the complete typed seed binding and the intermediate constructor
values needed for subtree reuse. Its maintenance rule reads the live canonical
marker and its seed, and runs in `Run!`. Consumers still require that same live
marker alongside the view. Retirement never revives a child Summary. Cached
interface rows can remain after retirement, but do not authorize a match alone.
Native union/rebuild enables new view bindings through the normal incremental
schedule; equality guards and correlated argument tuples remain in the producer.
These are seed views, not expansion of the entire saturated closure.

The joint solver charges each selected interface's schema and maintenance rule
once, plus each consumer's actual lowered queries. Shared/inline choices use the
same global layout variables as spine anchors and marker payloads. Independent
native parsing audits the emitted costs. With `--storage-cost instances`, native
checkpoint storage also includes interface rows, including stale rows. The
structural default does not estimate invocation frequencies, index bytes or RSS.

Effect elimination uses only the equations guaranteed by selected spine
initializers and support recipes. Within one RHS, an earlier retained insertion
or union can make a later effect redundant, never the reverse. A semantic
closure row alone does not prove a physical row exists, and equal constructor
results do not imply equal arguments.

This shares decoding across inherited consumers; it does **not** yet factor the
Cartesian product of all owner alternatives into one consumer. The late-bridge
example still has 62 transition/retirement rules and one shared maintenance rule.
See [the measured ablation](../../experiments/cell_spine/shared_interfaces/README.md).

## Solver and exact representation sharing

SCIP is the default, via the pinned `pyscipopt` wheel. It requires no Gurobi
license and has no licensed variable/constraint count cap. To compare the same
model with Gurobi, install `requirements-gurobi.txt` and pass `--solver gurobi`.
`--solver scip` makes the default explicit; `EGG_LAYOUT_SOLVER` also selects the
backend. There is no automatic backend fallback, MIP time limit, candidate cap,
or approximate solution acceptance. Every native solve must report OPTIMAL with
relative and absolute gap limits set to zero; numerical tolerances still apply.
SCIP is not claimed to outperform Gurobi on every model.

Before optimization, identical physical spine choices share one variable. The
key contains the certified closure family, complete seed facts/equalities,
ordered roots, typed ordered ports, and root identities in the family's certified
coordinates. Slot names may change; literal values, guards and wiring may not.
All original history entries remain transition obligations and native validation
cases. This removes redundant representation choices, not intermediate Cells or
observed paths. `--keep-duplicate-spines` restores the old choice space for an
explicit ablation. The report exposes the complete alias mapping.

Identical alpha-normalized conditional rule programs with the same full marker
layout domains also reuse a proven minimum. This cache applies only to structural
costs, not concrete instance-storage pricing. A reused `pricing.json` points to
the original proof model using `isomorphic_pricing_of`; no new solve is claimed.

`report.json` separates solver API construction from native solving, with model
counts, solve counts, peak variables/constraints, and engine versions. These
are subsets of the existing Bake phases, not extra durations to add to them.
`pricing_work` additionally reports preparation time, layout profiling time,
layout evaluations and exact-model cache hits. The first layout profile can
still enumerate a Cartesian product; model compression alone does not remove
that work. See [the measured full-window cases](../../experiments/cell_spine/scip/README.md).

`--layout-pricing projected` is now the default. For each transition, layouts
with identical read/create/retire recipes share a cost branch. All original
layout variables, storage-column costs, and emission choices remain in the
model. Read projections retain roots, decoding guards, binding fields and
raw/canonical storage; create projections retain construction/borrow recipes;
retirement needs the complete marker key. If a family has several roles their
requirements are combined. This is syntactic recipe equality, not sampled equal
costs or global candidate deletion.

Source layouts are visited first. Their owner bindings and LHS query obligations
are compiled once and reused for target layouts. Decoder deletion waits until
the target's variable uses are known. Final LHS normalization plans are cached
only under exact body equality, after decoder decisions and target-induced
lookups. Copies of mutable state isolate every continuation. Source snapshots
and normalization caches are released when that source tuple is finished.

For ablation, `--layout-pricing source-cache` enables only source/normalization
reuse; `--layout-pricing enumerate` recompiles every full tuple. All three use
the same final normalizer and require optimal MIP results. `pricing_work`
reports the original product, evaluated tuples, source compilations/reuses,
LHS normalization compilations/reuses, and per-rule layout choices before/after
projection. These count profile work, excluding final emission and native
validation. Whole-profile and whole-model cache hits do not fabricate additional
evaluations. See [reproduction and boundaries](../../experiments/cell_spine/layout_pricing/README.md).

Automatic Bake retains rooted capture for continuity with prior benchmarks.
Set `EGG_LAYOUT_CELL_WINDOWS=1` to include middle-history Cells during fresh Bake,
or pass a full `--analysis-dir`; supplied catalogs are never truncated.

## Composing already compact small Cells

`--cell-composition seed-cuts` is the default. After exact closure admission,
the compiler indexes certified single-root seed patterns by constructor name.
For every constructor in a known larger seed, it matches the available small
Cells on its immediate children. Two or more distinct child positions produce
an additional merge interface. For example, two Add2 instances and the actual
`Add(left,right)` connector can produce the known Add4 Cell. Operator names are
not special-cased. Typed bindings, repeated variables, literals and all explicit
seed equality guards must agree; remaining facts stay on the LHS. Guards are
checked against the initial seed, never justified by its future saturation.

These interfaces do not depend on the target's historical `composition.parts`.
An intermediate-window Cell can therefore become a parent where its certified
seed fits, even if the original trace recorded a larger prefix. Two instances
of one family share a representation choice but keep separate bindings. Source
templates reuse the exact representation normalization before matching.

The interfaces are separate obligations in the existing solver. Each keeps all
compatible endpoint representation choices and uses the same joint marker and
subtree-reuse cost model. Every admitted historical entry still receives native
validation. Exact ancestor views allow covered small markers to retire while
future uses are forwarded through the owner. Views that become identical after
certified field projection are merged before inherited consumers are expanded.

`transitions.json` labels these additional edges `certified_seed_cut` and records
the connector, source templates and complete port substitutions. They are
semantic decompositions of an already certified state, **not invented tier-0
apply events**. The Rust trace DAG/frontier is unchanged. `report.json` exposes
their count under `cell_composition`. Use `--cell-composition observed` (or
`EGG_LAYOUT_CELL_COMPOSITION=observed`) for the earlier parent-only ablation.

This is an immediate-constructor-boundary decomposition, not arbitrary subgraph
partitioning. New source fragments currently require one root; multi-root
sources retain their observed paths. A cut cannot bootstrap its own family.
No new closure is guessed for an unseen larger seed, and no candidate/time cap
is added. Alternate source contracts can still cause a Cartesian product and
inherited rules can grow. More supported interfaces do not guarantee faster
execution. See [native examples and results](../../experiments/cell_spine/seed_composition/README.md).

## Generated names

The compiler uses `Op!` for forked input constructors and `Op!!` for separately
materialized canonical constructors; `Term` becomes `Term!`. The two tables stay
separate so canonical construction cannot accidentally reactivate raw ingress.
Markers are `Summary-N!`, the marker sort is `Cell!`, and the generated rulesets
are `Run!` / `Export!`. Export mappings use `Reified-Term!`.

Only AST identifiers change. Literal strings, user rule names, and the original
rules remain untouched. The `!` suffix and helper names (`Summary-N`, `Reified-*`, `Cell` and
legacy `Spine-*`) are reserved in source declarations for this backend. Fresh Bake outputs use the new names; archived `.egg` and report
files are not rewritten. Native validation and table counters also recognize
legacy `Spine-*` names, so old measurements remain inspectable.

## Explicit debug snapshot coverage

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

- `snapshot.egg`: shared command encoding, with one `StageSummary` per
  checkpoint. This is an archive representation, not a generalized transition
  matcher. It does not materialize ordinary internal facts.
- `snapshot-dictionary.egg`: readable comments mapping command IDs to native `.egg`
  declarations/actions. `snapshot-library.json` is the machine-readable codebook
  and exact prefix/middle/suffix recipes.
- `snapshot-restore.egg`: expand the recorded states and perform checks/extractions in
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
using exact common prefixes and suffixes. The selected MIP solver minimizes weighted command
references and slice descriptors. There is no sampling, candidate budget or
heuristic solver fallback. The command dictionary cost is constant across these
choices. Search weight has no role in this mode: it is not a Join optimizer.
Archive bytes include the codebook and recipe `.egg`; no RSS/speed claim follows.
Some tiny programs are larger after encoding.

## Guarantees and remaining boundaries

`archived_stages` with `cellified:false` means **the recorded concrete execution is recoverable**, not
that its snapshots are universally interchangeable saturated Cells. Changing the
source/input requires rebaking. Equal table states can have different semi-naive
frontiers: those frontiers are not restored, and this is not a native continuation
snapshot. New bindings, arbitrary future grafts and continued native execution
are not covered by this stage contract. Use the exported final data for inspection;
parameterized execution remains separate work. The archive `compressed` flag
only compares the encoded recipe plus necessary codebook against unshared
checkpoint restore streams. It does not compare with original source bytes or
egraph RSS; negative savings remain visible.

The native program must finish on the supplied input. Nonterminating saturation
cannot produce a finite completed Bake. Proof generation, plugin-defined
commands/custom populated sorts without a codec, and external file writes are
rejected with an error, not silently copied or labeled compressed. Thus this is
broad standard concrete-program support, not a claim about every possible egglog
extension. Header or codec errors also prevent publication.

The original symbolic path still requires complete positive constructor/equality
certificates and one unconditional saturation boundary. It emits typed fork
nodes, independent Summaries, observed boundary rules and cached-closure export.
Opaque-boundary equivalence is a positive substitution guarantee, not a promise
that unseen concrete binding/union interactions are already saturated. An unsupported Cell lowering never redirects to the debug archive. The Cell
result uses `cellified:true`, not an unqualified compression claim. Solver/license errors remain errors. No engine changes are
introduced by either compiler.

## Direct input spine construction

The input MILP minimizes distinct constructor and marker recipes for the fixed
selected ABI, not raw `Add!` rows or the final runtime table count. Retaining raw
input can cost fewer recipes than constructing another certified interface.
Tests compare the chosen recipe objective with an explicitly constrained
zero-raw-Add alternative and validate both on the actual egglog runtime. The
alternative retains the strict zero-raw and size-reduction checks; unrestricted
selection reports all physical rows honestly, including retained raw rows.
Benchmark size and regression checks continue to use those measured counts.

Normal symbolic Bake now calls `input_spine.project` before writing the runtime
input. It matches certified seed patterns on the source AST's finite constructor
DAG; direct construction currently admits single-root, guard-free patterns.
A constructible match maps its typed parameters and root through
the existing closure-equivalence certificate to the **selected** canonical
spine; it never runs source inference or hard-codes an operator law.

For fixed spine/marker choices, a second MIP model selects initializer recipes
and retained raw rows. Every matched input region needs a selected initializer
covering it, possibly a larger one; unmatched facts remain raw. Constructor coverage alone
does not discharge a specialized binding: each matched Cell interface must be
provided by a certified ancestor lens, or its complete seed rows stay visible
to ordinary ingress. Multi-root and equality matches also contribute these
obligations even though they are not yet direct initializer choices. An original row
may be omitted only if a selected Cell covers it. Every retained parent and every
named global must still obtain each referenced value from a retained row or an
initializer's certified root alias. Opaque input subtrees remain raw. Existing
raw-row contracts and inherited-consumer support facts must also be constructed.
The cost shares equal constructor/marker expression recipes across initializers;
it excludes constant global bindings, indices, RSS, and union-dependent row
coalescing. It is not a runtime-time model or a global reoptimization of spine
choices. There is no largest-first heuristic, sampling, or candidate budget.

The emitted input directly constructs the selected forest and active marker.
It does not first insert the covered raw tree and delete it afterward. Named
intermediates needed outside the selected forest remain available; an external
`Use(inner)` is not discarded because `inner` occurs inside a larger Cell.
Constructor CSE keeps nested input writes concise. Source rule execution still
occurs only through the generated `Run!` ruleset; cached closure export is
unchanged. Whole-state native validation still gates publication of output.

`input-plan.json` records selection/retention and unsupported forms.
`input-selection.lp` / `.sol` record the initializer optimization when applicable.
Multi-root/equality-guard seeds and noncanonical storage layouts retain their
existing input route. Programs with explicit initial unions also retain the
original input order. This is current-input projection under the baked whitelist,
not an arbitrary-future-graft guarantee. Tests additionally cover a known late
boundary fact and a union-enabled late match on a retained intermediate.

The low-level `cellify(data, library)` API remains ordinary ingress when no
`initial=` recipe is supplied, so callers using an existing library with new
inputs do not silently assume they have a freshly verified projection.
See `experiments/cell_spine/input_spine` for native table counts and examples.

## Multi-parent spine composition

The full Summary argument lists below describe the logical compiler IR before
marker placement. Actual stored marker columns are in `retirement.marker_layouts`.

`fixtures/dual_parents.egg` bakes three exact families. `A(x)` derives `AA(x)`;
`B(x)` derives `BB(x)`; both hidden facts enable `Joined(x)`. Automatic lowering
now produces this **actual rule**, rather than factoring just the first parent:

```egg
(rule ((Summary-0! parent_0_root_0 p0)
       (Summary-1! parent_1_root_0 p0))
      ((Summary-2! parent_0_root_0 parent_1_root_0 p0)
       (subsume (Summary-0! parent_0_root_0 p0))
       (subsume (Summary-1! parent_1_root_0 p0)))
      :ruleset Run! :name "impl_2_0")
```

`fixtures/nested_parents.egg` additionally checks `A + B -> AB`, then
`AB + C -> ABC`: the combined spine is an ordinary parent for later baked
transitions, not a terminal archive.

Both parent spines remain shared. Their common `p0` is the binding constraint;
family identity alone cannot enable this transition. Neither `AA`, `BB` nor
`Joined` is materialized as a forked internal node. The larger Cell's Summary
refers to both existing roots; its cached closure is expanded only in
`Export!`.

`fixtures/shared_family_graft.egg` demonstrates **two instances of one family**:

```egg
SummaryA(root0, x, y) + SummaryA(root1, y, z) + Gate!(x, z)
    -> SummaryCombined(root0, root1, gate, x, y, z)
```

Instance identities and port mappings are distinct even though the two Summaries
use the same relation and solver-selected representation. The external `Gate`
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
This couples both parents and the target. Two unrelated pairwise edges would
lose shared binding constraints. The implementation is input to the certified
ancestor-takeover pass described below.

For each alpha-local template shape `n`, `y[n]` is the OR of all selected
representations using it. Objective:

```
storage_weight * (sum(y[n]) + selected_marker_columns)
+ search_weight * actual_runtime_reads_and_guards
+ update_weight * actual_runtime_writes_unions_changes_and_marker_columns
```

For each full path/implementation choice, first generate all inherited consumers
and retirement rules. Lower each affected rule with **both endpoint marker
layouts**. The lowering reuses certified source subtree values, constructs the
target canonical spine, and removes unused decoders where proved safe. Its final
AST supplies the cost. A marker carrying a reusable subtree can therefore save
reads and writes in later transitions, at the cost of an extra column.

Reads count distinct nested calls and remaining equality guards. Updates count
distinct nested RHS calls, unions, `subsume` actions, marker creation columns,
and the columns needed to identify a marker for subsumption. Repeated syntax in
one rule is counted once for calls. This is a structural proxy, **not measured
Join cardinalities, runtime frequency, committed mutations, time or RSS**.
Shared template shapes also do not imply shared runtime instances. Explicit
export, source input construction and rule installation are outside this score.

The solve is conditional but not greedy:

1. An outer MIP model chooses paths and transition implementations.
2. An inner MIP model jointly chooses marker layouts and legal physical
   representations (canonical, borrowed, or boundary form), and returns the
   minimum cost of the actual generated runtime, including inherited rules.
3. An exact conditional value cut feeds this cost back to the outer model.
   It can choose different paths. Stop when its lower bound reaches the best
   feasible total cost.

The cut is `extra >= residual * (sum(selected_decisions) - count + 1)`.
For this complete choice it charges its exact additional cost. For another
integer choice the RHS is nonpositive. The residual subtracts the minimum marker
storage already charged by the outer model. No guessed large constant is used.
Each inner problem is solved to optimality; otherwise compilation fails.

Each rule's joint marker-cost table is represented by a reduced multi-valued
decision diagram. Identical suffixes share cost nodes. The only binary variables
are marker choices; diagram costs are continuous, so there is no binary variable
for every marker pair. Alpha-equivalent rule recipes with identical typed marker
recipes reuse profiles across path IDs. Recipe equality, not sampled cost
similarity, authorizes this reuse.

The final runtime is emitted from the same prepared rules used by the solver.
An independent native-parser pass checks that the entire rule-name set and each
rule's cost equal the priced AST. `retirement.cost_audit` records that check;
`plan.pricing.rule_costs` also lists the subtrees actually substituted by values.
Optimality is for the supplied paths, legal implementations and layout forms,
under this structural score. It is not a global optimum over arbitrary spines
or arbitrary marker payload subsets.

Weights default to 1 and are explicit CLI arguments. There is no candidate
budget, sampling or greedy fallback. No search over arbitrary parent subsets is
introduced: all certified parents of the observed composition are used. For
hyperedge `e` touching distinct endpoint families `F_e`, the number of compiled
options is `product(f in F_e) |P_f|`; total options sum over observed hyperedges.
This can still grow combinatorially with representation alternatives. The solver's
optimality is only for this finite option set and proxy cost. Solver/license
errors are surfaced, not worked around by quietly dropping options.

For a rule mentioning marker families `F`, profile construction can require
`product(f in F) |L_f|` lowerings. Cost-diagram compression reduces solver size,
not this worst-case enumeration cost. The outer solve may visit every legal
path/implementation assignment. Its conditional cuts also grow with that count.
This is exact finite optimization, not a polynomial-time or universal
tractability guarantee. Per-model variable and constraint counts are
reported separately for the outer model and peak inner model.

`selection.lp`/`.sol` expose the outer model and certified solution.
`work/pricing/scheme-NNNN/selection.lp`, `.sol`, and `pricing.json` expose each
conditional marker solve. `transitions.json` records all pre-lowering options,
port maps and required representations; its old raw AST counts are diagnostic,
not the objective. `report.json` lists selected implementations and final costs.
`retirement.costed_transitions` retains the pre-takeover implementations. Concrete stage
recipe selection remains a separate, per-stage problem. `StageConcat` is still
archive sharing and is not used as a substitute for these Cell transitions.

`experiments/cell_spine/joint_pricing` compares frozen old choices and the joint
solve using the same emitter, cost formula and native full-state verifier.
It includes the long-spine graft where the optimal marker moves onto a reusable
subtree, plus the sparse eggcc If subset. It reports per-model variable/constraint counts.

`experiments/cell_spine/joint_runtime` adds native old/marker-only/joint ablations
on identical catalogs. The structural score improves the long graft but slows
sparse If inference: fewer syntax terms do not imply fewer Join operations.
It reports full-graph equivalence, canonical as well as raw storage, library
sizes, per-phase timings and existing native probe counters. Its separate
seven-input AC probe records an unfinished analysis rather than claiming a
compressed result.

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

Summaries are now constructors returning `Cell!`, so egglog can subsume a
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
small Summary arriving after its covering owner. Under the generated phased runtime schedule,
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
The MIP objective prices these forms after redundancy removal and marker
lowering, so they now affect path and marker selection. `retirement` exposes projections, retained cycles,
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

## New-graph time/RSS benchmark

```sh
out/cell-spine-venv/bin/python tools/bench/cell_spine.py \
  --output out/cell-new-graph-bench-next --copies 1 1000
```

This reuses the repository's independent-process measurement and exact Math graph
comparison helpers. Each tiny source is baked once, then reused on fresh literal
bindings in a new egraph, without analyzing the test graph. `native`, `spine`, and
`spine_export` use the same current native runner; an existing SHA-verified
pristine binary is included as a separate reference (`--no-pristine` omits it).
Timing and RSS runs do not dump graphs; exact verification uses other processes.
Installation, input construction, execution and full export are separate phases.
AST-to-spine conversion, offline compilation and library bytes are also reported.
One run per case, no averaging. See `experiments/cell_spine/new_graph_bench` for
results and limitations: this is a four-rule workload with disjoint new instances,
not full math-microbenchmark generalization or a demonstrated end-to-end speedup.

## Sparse eggcc If subset

`tools/bench/eggcc_if.py` runs the actual If-to-min/max rules with all constructor,
index, type and context conditions intact. The shared phased native harness now
supports typed constructor graphs; pristine and current builds use the same
module. `experiments/cell_spine/eggcc_if_tiny` records the source revision/license,
matching and near-miss inputs, exact all-sort graph comparisons, and timings.

This workload exposed a missing connection: a parent may prove that a hidden row
(e.g. Get(Single(x),0)) equals a visible Summary field. `visible_projections`
uses the parent's actual closure snapshot to replace such LHS reads with the field;
it does not hardcode operators or assume constructor injectivity. Direct root
aliases take precedence, and children are projected before the outer expression.
The RHS keeps its canonical materialization contract. Native regressions verify
that min/max results are recovered while near misses remain unmatched.

## Split independent scopes before Bake

```sh
out/cell-spine-venv/bin/python -m tools.cell_spine.split --source /path/to/math.egg
```

This parser-only command writes one `math.split-NNN-NAME.egg` per independent
execution scope and a `math.split.json` report **beside the selected source**.
It uses egglog's native parser through `examples/split_ast.rs`, preserving raw
source text, wildcards and adjacent annotations. `--dry-run` writes nothing;
`--skip-build` uses the existing native parser binary.

Scopes containing direct schedules are exported regardless of their ruleset
count. Inherited prefixes are replayed, completed sibling scopes are omitted,
and ancestor push wrappers are removed. Each file describes its scope before
pop. All scheduled rulesets, finite counts, `:until`, checks, command order and
effects are preserved. Only rulesets never scheduled on that path are removed.
Nested and counted push/pop are supported. Empty wrapper scopes are not extra
examples. Invalid stacks and unsupported includes, combined-ruleset/custom/proof
commands or external file IO fail explicitly.

Splitting is separate from Bake admission. Each v2 report entry lists `rulesets`
and a static `bake.status`: `single_ruleset`, `requires_multi_ruleset_support`,
or `no_scheduled_ruleset`. The first is only a ruleset check; compiler/native
validation is still required. Multi-ruleset files are emitted, not skipped.

All outputs are reparsed and their retained command texts compared in order with
the originals. Identical reruns reuse existing files, and conflicting edits are
never overwritten. An exact, unmodified v1 manifest for the same source can be
migrated to v2. If the source changes, a separate deterministic source-hash batch
is emitted in the same directory, preserving prior outputs and their manifest.
The operation does not execute rules or certify saturation.

The complete upstream web-demo `math.egg` produces 21 files: one `add-ac` scope
and 20 scopes retaining default + `prune`. The unchanged add-ac scope's original
7 rounds and check pass native execution and concrete-stage Bake. This is not
proof of successful Bake for every other mixed program. Explicit stage export
supports multiple native rulesets; the actual default/prune scope also passes
checkpoint reconstruction. Strict symbolic Cell composition remains single-ruleset.

```sh
cargo test --offline --release --example split_ast
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_split -v
```

## Multiple rulesets in explicit snapshot export

The checkpoint runner executes each original RunSchedule on the same native
egraph, with its ordinary per-rule incremental frontiers. It preserves seq,
repeat, :until, prune, union, updates and scope rollback. No rulesets are merged
and no schedules are reordered. Thus explicit stage export needs no single-ruleset gate. Default Cell Bake
keeps its separate admission checks.
Restore remains immutable checkpoint reconstruction, not restoration of the
native semi-naive frontiers for arbitrary continuation; new inputs require
rebaking. Symbolic Cell compilation keeps its separate single-ruleset contract.

`fixtures/stages/multi_rulesets.egg` checks that running prune does not reset the
default rule's frontier and accidentally repeat a non-idempotent merge. Native
checks, hidden rows and rollback states are verified again after restoration.

## General reuse limitation

The existing three-leaf library restores its own left-associated input exactly,
but not arbitrary right-associated or larger inputs. See
`experiments/cell_spine/reuse_boundary/README.md` and its native reproducer.
The missing capability is successor matching over certified closure rows that
are absent from the physical spine. No dedicated AC algorithm is used to hide
this limitation. Default Bake fails rather than returning a snapshot as success.

```sh
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_contract -v
```


## Active canonical markers

`markers.py` is the runtime lowering. The old `frontier.py` was removed. There
are no global `Read-*!` or `Wanted-*!` relations in generated programs.

- `Op!` is raw ingress. Original inputs and new external facts use it.
- `Op!!` is the selected spine. Generated inference never writes raw
  ingress constructors, so normalizing a Cell cannot reopen arbitrary ingress.
- `Summary-N!` is the **active marker itself**. Its fields and attachment
  point are selected by the MIP solver. There is no second summary-record layer.

A transition requires active parent markers. It reads canonical nodes only
through these markers. Known subtrees become result variables in both LHS and
RHS. A transition needing only a stored root does not decode the old chain.
It stores the changed part according to the selected contract, publishes the new
marker, and subsumes
certified parent markers. Normalization can read noncanonical input; subsequent
inference is authorized by canonical markers. Merely inserting an unmarked Cano
node does not activate a Cell.

For the ordinary `long_spine_graft.egg` fixture, the main graft has two LHS atoms:

```egg
(rule ((Summary-0! root) (= next (Extend! root extra)))
      ((Summary-1! next root)
       (subsume (Summary-0! root)))
      :ruleset Run!)
```

This selection borrows the existing raw chain and Extend row. There is no
canonical copy; explicit export reads that chain through its active owner.
The fixture has a seven-input chain and an external graft. It is **not** the
complete seven-input AC problem. Actual generated names and inherited consumers
are in the report and `library.egg`.

### Placement and field selection

The MIP solver jointly selects canonical paths, transition implementations and marker
layouts. Every canonical constructor and opaque input can be an attachment
point. Equivalent stored-field/decoder layouts are deduplicated. Each point
supports three explicit payload forms: owner roots plus anchor; also carry
inputs outside the anchored subtree; or carry all inputs. This is a defined
layout space, not a claim to enumerate every possible interface encoding.

Owner roots remain in the marker, so two Cells sharing a tail do not share an
activation accidentally. If fields are omitted, the decoder checks the canonical
seed forest and its equality guards. Reading one root alone is insufficient:
union of constructor outputs does not imply union of their inputs. Dead decoders
are removed only when their omitted bindings cannot affect the transition; joins
between multiple owners keep their correlation checks unless a single owner
exposes all subordinate forests directly in the same physical table namespace
and the subordinate markers carry roots only.

The objective includes shared template shapes, stored marker columns, and actual
lowered reads/writes and marker keys, including inherited consumers and decoder
elimination. The conditional solve above feeds those costs back into spine
selection. `gate_audit` and `cost_audit` check the emitted code. Solver optimality
refers to this structural model, not actual time or RSS. The cost diagram shares
equal suffixes without sampling layouts or imposing a candidate budget.

### Coalescing repeated constructor lookups

After marker lowering and variable normalization, `normalize_lookups` merges
queries with the same constructor and argument tuple. Their result variables
become aliases throughout the rule. It repeats to a fixed point because merging
child results can expose duplicate parent queries. Both `Op!` and `Op!!` are
supported, but remain different tables.

At least one constructor-existence check remains. Different argument tuples,
argument orders and literals are preserved; equal results never imply equal
inputs. RHS unions are not used to justify LHS simplification. Primitive or
undeclared/nonconstructor calls are outside this pass. Only resulting unions
between the same bound variable are removed; constructor-bearing actions may
still create facts. Pricing and emission use this same lowered AST.

The inherited `long_spine_graft` continuation now queries `Extend!` once and
publishes its target marker without a redundant union. `test_lookups.py` checks
cascading aliases, guards and negative cases; `test_markers.py` checks the actual
emitted rule and the native closure through the normal compiler pipeline.

### Solver-selected physical representation

`representation.py` extends each conditional path choice with legal storage
contracts. These are inputs to the cost diagram, not patches to a solved program.
All producers, inherited consumers, retirement rules, and export agree on one
marker ABI for a family.

- **Canonical** keeps the previous behavior: store the complete canonical seed
  forest, with optional parameter decoding.
- **Borrowed** stores no canonical copy of that forest. Its raw rows must exist
  on **every** producer path. Decode them under the active marker when needed.
- **Boundary** stores one immediate canonical subtree. The existing raw root
  need not be duplicated; the marker also carries inputs outside that subtree.
  If the subtree is independently guaranteed raw, it may be borrowed too.

Boundary form currently requires a single root, a single compound child, and no
seed union guards. Multi-root forests retain full correlated decoding. This is
an explicit finite representation space; it does not enumerate every subset of
nodes or change the rules to special-case Add, If, or associative laws.

The frontend checks raw-row availability as a finite inductive invariant over
**all** producers, including inherited/self transitions. It starts with seed
rows and removes obligations that any producer cannot preserve. Matching a
constructor's output never implies equality of its arguments. Logical seed
identity alone is not evidence of a raw or canonical table row. The lowerer
therefore tracks canonical-row availability separately, still constructs a
canonical child when a borrowed parent cannot supply its physical readers, and
preserves required unions between existing roots.

`retirement.raw_contracts` lists the admitted raw rows.
`retirement.marker_layouts` records each selected `representation`,
`read_storage`, and `stored_roots` (absent fields mean canonical for compatibility).
`plan.pricing.rule_costs` prices the resulting reads and writes, so a removed
canonical copy reduces the actual update score. `template_nodes` still counts
shared **descriptions**, including export recipes, not runtime physical rows.
The structural objective does not model failed Join-prefix cardinalities, so a
smaller score is not a speedup guarantee; sparse If remains a counterexample.

### Current-input instance storage in selection

Select `--storage-cost instances` to include current-input storage. For each potentially
winning spine/implementation/marker assignment, it recomputes the existing exact
initializer row-cover solution, executes only that compact program in native
egglog, and prices its actual retained function-table Value slots. The score is:

```
storage_weight * (template_nodes + marker_columns + interface_columns + checkpoint_peak_value_slots)
  + search_weight * lookup_atoms_and_guards
  + update_weight * write_atoms_and_unions
```

`checkpoint_peak_value_slots` is the maximum at the input and post-saturation
checkpoints. Each row includes inputs, output, timestamp and the subsume flag
when applicable. Native hashcons counts a shared row once; different bindings,
required raw input, leftover canonical rows and retired markers are counted.
`subsume` does not subtract a row. No second per-instance charge is added on top
of the marker/binding rows. This remains a structural storage score, not bytes,
allocated capacity, RSS, a within-sweep peak or a runtime-frequency model.
Actual materialized index shapes are recorded but index bytes, hashcons capacity,
union-find, stale backing rows and temporary buffers are explicitly unpriced.

Nonnegative storage and mandatory raw-input rows yield a valid lower bound.
For mandatory rows, e-class arguments and numeric literals are collapsed when
counting keys, permitting extra coalescing and hence only undercounting.
Distinct String keys/tables and retained globals provide the remaining bound.
A measured layout adds an exact value cut; a scheme whose lower bound cannot beat the incumbent does not need
native measurement. No candidates are sampled/truncated. This can still be
expensive: `instance_storage.evaluations` and `.seconds` report that Bake cost.
SCIP is the default; optional Gurobi remains subject to its license. Errors
never fall back silently to the old objective.

This is optimization **under the existing initializer policy**, not a global
search over every initializer. For each ABI, that policy minimizes initial
distinct constructor/marker recipes. Tied initializers, or a more expensive
initializer that would save later storage, are not exhaustively searched.
Spine/marker choices now see the storage produced by that policy. The priced
initializer is emitted verbatim, and native full-closure validation additionally
checks that every emitted pre-export table row count equals its priced count.
No export rules or original inference rules execute during compact pricing.

`--storage-cost structural` remains the default: exact instance pricing may
require many layout evaluations on larger catalogs, so this integration does
not silently impose that Bake cost on every existing caller. Historical
structural tests/experiments pin this mode. Native storage tests and a separate
CI ablation exercise instance pricing. The report's
`instance_storage.selected.native` contains both checkpoints and per-table
counts. A lower-bound-only pricing result is never reported as a measured plan.

New raw facts continue through existing ingress; borrowed storage is never
written by inference. Input union preserves these rows modulo rebuild in the
supported positive-constructor subset. Deletion, user subsumption of borrowed
input, mutable function tables and arbitrary foreign writes are outside this
backend's contract. The original canonical mode remains available in the same
optimization problem.

See `experiments/cell_spine/representation` for the native single-run comparison
and `test_representation.py` / `test_markers.py` for mixed-storage, missing-row,
noninjectivity, correlated-join and required-union checks.

### Retirement and validation

The compiler also proves containment at actual canonical spine positions. Thus
a large Cell can retire a small marker at its root as well as in a child subtree.
Retired markers do not authorize matches. Larger owners inherit the already
baked continuations. Canonical storage can remain shared with other instances.
`subsume` stops matching; it is not physical reclamation of the retired row.

`gate_audit` rejects canonical lookups without an active marker and generated
writes to raw ingress. Native tests cover late facts, input union, noninjective
bindings, shared tails, output-only union, and multi-parent continuation.
All constructors **including Cano constructors**, auxiliary rows, and subsumed
markers are counted before export.

This backend is a baked-transition whitelist, not a replacement exhaustive
matcher over every hidden closure row. In particular, the previously frozen
three-input library is not promised to close every unseen seven-input AC graph.
The separate `reuse_boundary/probe.py` retains that negative generalization
check; a mismatch is reported as rejected, never as compression. A successful
source Bake still requires whole-state native equality, including all original
constructors and equality classes, before executable output is published.

Reproduce the current-input and graft checks:

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/canonical_markers/run.py \
  --output out/canonical-markers-new --skip-build
out/cell-spine-venv/bin/python -m unittest discover -s tools/cell_spine -t . -v
```
