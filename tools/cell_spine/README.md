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

## Binary optimization

For exact Cell family `f`, choose one observed path `p` using `x[f,p]` and
`sum_p x[f,p] = 1`. For each shared structural template node, `y[n]` is the OR
of all choices using it. For observed parent-family edges, `z[p,q]` is the AND of
the selected endpoint paths. Objective:

```
storage_weight * sum(y)
+ search_weight * sum(path_LHS_atoms * x)
+ update_weight * sum(new_template_nodes(p,q) * z[p,q])
```

Weights default to 1 and are explicit CLI arguments. Sharing refers to
alpha-local **template code shapes**, not automatic identity of different
runtime instances. Update is likewise a shape-difference proxy, not a claim that
two runtime bindings share values. This is a global coupled model, not an
independent shortest-path choice, but it optimizes this stated proxy rather than
measured Join time, RSS or arbitrary unobserved paths. The model contains all
admitted observed paths; no candidate budget, sampling or greedy fallback.
License/model errors are surfaced. Only proven optimal solutions are published.

Parent factoring uses actual history token mappings plus seed containment. It
replaces the parent subquery with a Summary and keeps residual coarse facts and
all equality obligations. It does not search arbitrary pairs of Cells. Rules
with identical ingress/head are deduplicated, retaining a valid factored form
when available. The selected constructor forest remains visible; Summary is a
relation and does not change its hashcons key.

## Verification

```sh
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_spine -v
```

For both paths:

```sh
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_spine tools.cell_spine.test_stages -v
```

Tests include global sharing versus independent minima, exhaustive verification
of a tiny weighted model, actual native analysis on multiple programs, ordinary
row isolation, full cached-export equivalence, delayed boundary grafting,
non-injective binding, and negative tests for wrong union, ordinary-node leakage
and secretly running original rules. Relation/function stage compilation is executed on native egglog, together with
upstream list and merge programs. Mutated function values and lost subsumption
are rejected by the independent witness checker:

```sh
target/release/examples/spine_stages --verify out/my-spine-run/stage-bake/stages.json
``` These are correctness checks, not a speed/RSS benchmark.
