# babble adapter

This standalone Cargo workspace calls upstream babble's `BeamExperiment` and
`apply_libs`. It is not a reimplementation of library learning. Root egglog and
its dependencies are unchanged. No domain equations are supplied.

## Reproduce

From the repository root, with Python supporting tarfile's `data` extraction
filter, curl, and Rust installed:

```
python3 tools/babble_adapter/scripts/run.py
```

The script downloads checksum-verified source archives into ignored `tools/.deps`,
exports the existing math source-witness corpus using egglog's native parser,
builds with the adapter's committed Cargo.lock, and runs each learning process
with a 180-second timeout. No commercial solver is enabled.

Pinned source revisions:

- https://github.com/dcao/babble/tree/115f920db52f124fd7247d1f80e811f3784e6896
- https://github.com/dcao/egg/tree/caf623cad25cacba83193464dcf0d3dd4161ba7a

The second revision is the egg fork pinned in upstream babble's Cargo.lock.
Bootstrap records two changes to downloaded babble: its egg dependency is redirected
to the local pinned archive, and `Self(node)` inside a nested generic function in
`ast_node/expr.rs` is replaced by `Expr(node)` for current Rust compatibility.
Learning algorithms are unmodified. Upstream license files remain in the downloads.

## Input and correctness boundary

`src/bin/babble_corpus.rs` consumes existing `binding_program/math.json` plus the
matching native `profile.json`. It emits 124 unique combine classes, with their
normalized wiring, producer and consumer contracts, external consumer ports,
and source connection strings. Rules are parsed by egglog, not another parser.
Ports are named by their existing producer/consumer roles. Unknown action forms
remain opaque strings, never assumed to be positive effects. This corpus is not
a complete event DAG and does not contain an automatically extracted bridge plan.

The corpus preserves conditions and union action syntax; this adapter does not
infer committed effects from the syntax. Local variables live within their
individual rule contracts. Binding ports are data, separate from babble's own
lambda binders, so input `$0`/library-looking tokens cannot capture learned binders.

Each encoding round-trips to the original JSON. An independent lexical evaluator
expands learned functions and checks exact equality with every encoded input
program, including the held-out programs. No learned macro is applied as a new
runtime rewrite. Library parameters are syntactic holes, not automatically
certified typed relational interfaces for arbitrary new arguments.

## Evaluation

- Train: 99 classes; test: every fifth class, 25 classes. Occurrence counts are
  retained as metadata but not duplicated or used as weights in learning.
- This is a within-trace split, not independent-run or unseen-rule generalization.
- Train selected library rewrites are applied to test inputs without new learning.
- Settings: beam 16, at most 2 selected libraries per step, max arity 3, 2 library
  rewrite iterations. `learn_constants=false`. There are zero domain equations.
- Cost is AST nodes in each encoding, including library definitions and calls.
  JSON-control and typed-AST have different raw sizes; compare each to its own raw
  baseline. Exact whole-program dictionary cost is reported separately; an
  encoder could always fall back to the cheaper raw representation.
- These metrics do not include all symbol bytes, graph metadata or runtime memory.
  Whole-program dictionary sharing is not a full hashcons/DAG storage baseline.
  No claim of e-graph memory reduction, recursive-family proof, or shortcut validity.

Artifacts: `experiments/babble/corpus.json`, `json-control.json`, `results.json`.
Reports include definitions and syntactic reference counts in train/test corpus
bodies (not invocation counts after recursively expanding dependent definitions).
Large `.programs.json` files are regenerated and ignored by Git; they contain
complete learned programs for inspection. Internal library IDs/tie selections may
vary across runs, despite pinned sources and a deterministic corpus split.

Tests:

```
CARGO_INCREMENTAL=0 cargo test --bin babble_corpus
cargo test --release --locked --manifest-path tools/babble_adapter/Cargo.toml
```

The `au-reference` mode exports raw AU candidates and match signatures directly
from upstream for the native egglog differential test. It skips library selection.
Run `python3 tools/babble_adapter/scripts/compare_egglog.py`; the scoped replacement
experiment is documented in `experiments/egglog_babble/README.md`.

`candidates=PATH` selects the hybrid route: read the native candidate JSON, turn
its binder-free patterns into library rewrites, and use upstream beam selection
and extraction without running upstream candidate generation again. Candidates
are ordered as upstream PartialExpr values to preserve beam tie behavior.
The comparison script runs both complete routes, checks costs and selected
definitions, and verifies train/test expansion. Co-occurrence reference flags in
the fixture are now comparison-only; native egglog computes them from roots/edges.
