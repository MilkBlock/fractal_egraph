# Automatic Cell spine compilation

```sh
python3 -m venv out/cell-spine-venv
out/cell-spine-venv/bin/pip install -r tools/cell_spine/requirements.txt
out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/fixtures/assoc.egg \
  --output out/my-spine-run
```

The default command builds the native tools, runs the existing `egg_layout
analyze` pipeline with graphical output disabled, reads its **completed exact
Cell catalog**, solves path selection with Gurobi, generates a forked `.egg`, and
checks it on the actual local egglog runtime. No new parser or engine changes.
Use `--analysis-dir DIR` to reuse an analysis of exactly the same source;
`--skip-build` is for already current native binaries. Output must be empty.

Outputs:

- `library.egg`: unchanged original rules, shared fork constructors, independent
  Summary relations, normalized paths, parent-Summary boundary transitions, and
  cached-closure export rules.
- `runtime.egg`: library plus automatically forked source constructor actions.
  It executes only `Spine-Run`, retaining multi-node structure and shared prefixes.
- `restore.egg`: runtime plus `Spine-Export` and transformed source checks.
  **Export publishes cached saturation facts; it does not run the original
  ruleset again.** It intentionally materializes ordinary rows when requested.
- `selection.lp`, `selection.sol`: inspectable Gurobi model and optimal solution.
- `report.json`: chosen paths, rejection/fallback reasons and observed parent
  transitions. `validation.json`: per-case physical row counts and full native
  equality/constructor comparison.

Executable artifacts are published only after native checks pass. The
validation library is prepared once and cloned per case; the manifest stores
one library and paths to existing replay entries, not copies of full histories.

## Guarantees and limits

The entry point accepts `.egg` input but **does not promise to compress every
single-ruleset program**. Compression currently requires positive constructor /
equality certificates, one rule-bearing ruleset, one unconditional saturation
boundary after initial actions, and ordinary datatype fields. Multiple equality
sorts, literal fields, explicit unions and repeated parameter bindings are
supported. Function/relation effects, containers, finite-round schedules, early
checks, late initial actions, observation-only snapshots and unfinished Cells do
not silently receive these semantics. Unsupported input is copied byte-for-byte
to `runtime.egg` and reported as `native_fallback`; it is not a compression win.
Invalid generated code or failed differential validation is an error, not a
silent fallback. A saturating source program can itself diverge, just as in native
egglog. Existing analyze/ripen limits retain their existing meaning.

Every admitted observed entry is compared with its generated spine and cached
export. The complete input program is compared too. Comparison includes **all
original constructor rows, typed literals and the equality partition**; original
source checks retain their root bindings. Ordinary constructor tables must be
empty before export. The validator rejects execution of any original ruleset on
the compressed path. Read-only table inspection does not construct probe nodes.

Opaque-boundary closure equivalence licenses positive substitutions. An actual
binding/union can enable additional interactions beyond the baked whitelist;
there is no claim of complete inference on arbitrary future grafts or of dynamic
rebaking. Summaries provide the baked facts, not an assertion that every concrete
context is permanently saturated. Unsupported hidden touchpoints are not
invented. There is no automatic deletion/subsume of a pre-existing original
shared graph: ingress transforms initial constructor actions before that graph
is materialized.

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

Tests include global sharing versus independent minima, exhaustive verification
of a tiny weighted model, actual native analysis on multiple programs, ordinary
row isolation, full cached-export equivalence, delayed boundary grafting,
non-injective binding, and negative tests for wrong union, ordinary-node leakage
and secretly running original rules. Relation/function fallback is also executed
on native egglog. These are correctness checks, not a speed/RSS benchmark.
