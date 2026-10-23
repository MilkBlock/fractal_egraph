# Standalone structural keys and the source pipeline

The Analyzer branch already **implements** structural keys. Its `--structural-keys`
mode inserts helpers and rewrites eggcc's known selection shapes; it is not merely
a warning about needing a key. However, its `normalized.egg` depends on a custom
Rust `CellStructuralKey` type and primitives. This fork supplies a standalone
source representation, then connects the existing source marker transformer and
pristine/GPU comparison harness:

```
source.egg → source_sk.egg → source_sk_mk.egg
             │                    │
             ├── pristine         ├── pristine
             └── GPU              └── GPU
```

Reused policy: `eggcc-structural-keys` commit `a89c4cc`,
`stable_choice::{rewrite_ties,setup_full,schedule_full}`. Base of this isolated
worktree: `95e6e44`. The original faster to-hoist-only API remains the default;
there is no silent new policy in the program runner. The `egglog/` kernel and
immutable `egglog-baseline` tag are unchanged.

## Representation and exactness

Every constructor sort receives a `CellStructuralKey-Sort` function. Its value
is a native `Vec<BigRat>` containing `[code, scale, height, anchored]`, not a
custom Rust sort. A typed structural witness is serialized as a prefix-free
base-258 word. Concatenation uses exact arithmetic:

```
(code_a, scale_a) · (code_b, scale_b)
    = (code_a + scale_a * code_b, scale_a * scale_b)
```

The order is the reused policy's anchor/height/typed-label/children order.
Numeric comparison of complete words equals lexicographic structural comparison;
there are no hash collisions or floating-point tolerances. Dynamic i64 labels
use their signed decimal representation, including i64::MIN. Input witnesses are
frozen once, after a recognized empty-LHS initialization. Subsequent constructors
receive keys and union merges keys by the minimum.

The exact source policy implements `to-hoist`, the equal-cost `Smaller` choice,
`IVTMin` ties and late equal-cost `ExtractedExpr` improvements. It deliberately
changes the old allocation/order-sensitive choice. **Original vs SK equivalence
is not the acceptance contract.** The oracle compares this emitted source with
the previously implemented Rust structural-key policy. That older policy itself
only claims selected-program equivalence, not deterministic intermediate graphs
for arbitrary nonconfluent programs.

BigRat computations run in explicit `sk-portable`/`sk-portable-freeze` rulesets.
GPU mode keeps those sets on the native CPU; original supported queries and
marker queries still use GPU. Their output includes ordinary i64 comparison
rows. Both backends consume the exact same source and schedule. No hidden key
injection takes place in the runner.

## Use

From this checkout, build and generate all three sources, then compare the four
actual runtime lanes (Python 3.9+, solver environment with SCIP or Gurobi):

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py structural-pipeline \
  --source experiments/standalone_structural_keys/add-pass3.egg \
  --analysis experiments/analyze_library_credit/library \
  --output out/standalone-keys-add \
  --worker out/worker/matrix --check
```

`--solver-python /path/to/python` chooses the solver installation. `--no-build`
uses already-built binaries; pristine is still fingerprint-verified. Omit
`--check` for source generation only. Output directories must be new. A failed
stage retains its logs and all prior artifacts; `pipeline.json` records commands,
source/library fingerprints and the failed stage. Unchanged backend lanes reuse
the existing comparison cache.

Only the first step, without marker selection:

```sh
cargo build --offline --release --target-dir target/analyzer \
  --example structural_key_source --example lhs_ast
/opt/homebrew/bin/python3 -m tools.cell_spine.structural_keys \
  --source experiments/standalone_structural_keys/hoist.egg --output out/standalone-keys-only
```

The generated source runs on pristine egglog without this Python/Rust compiler,
custom sort registration, Analyze JSON or a GPU worker. The compiler uses the
native parser/typed preflight; the small AST printing adapter preserves quoted
strings rather than introducing another parser.

## Limits that remain explicit

- This is the existing **eggcc selection policy**, not automatic inference of
  every user merge's desired tie behavior.
- Flat source, declarations/rules before inference, and ordinary source literals
  are supported. Includes, stack scopes, mutually recursive `datatypes` groups,
  and computed String/f64 primitives are rejected. Instantiating a constructor
  with an unsupported container field fails rather than falling back to IDs.
- Helper progress participates in the transformed schedule, so this is not a
  proof that an arbitrary original finite Run or nonconfluent saturation is
  unchanged. Marker maintenance has its own analogous schedule caveat.
- Exact rational words can expand DAG witnesses and become expensive. This is a
  portable correctness/reference encoding, not a compact DAG or performance win.
- The second source step reuses `pass3_markers`: Analyze-witnessed **two-atom,
  full-key** constructor fragments. The newer symbolic solver supports larger
  spines/optional fields but emits a runtime companion plan, not standalone
  optimized `.egg`; this work does not pretend those two backends are identical.
- Existing Analyze libraries are inputs, not silently rebuilt or promoted to
  new closure guarantees. GPU fallback/admission boundaries remain explicit.

See `RESULTS.json` for the checked fixtures and actual runtime comparisons. A
full raytrace source was generated, but full raytrace SK execution has **not**
been validated here.

## Verified checks

| Workload | Rust policy → standalone key association | SK / SK+MK × pristine / GPU |
|---|---:|---|
| Real eggcc `add` Pass 3 | 209/209 keys, 238 original rows | All four comparisons equal |
| Active publisher + three consumers | Covered by explicit source checks | All four comparisons equal; 1 marker row, all 3 consumer rows |
| Strings, floats, booleans, Unit, i64 extrema | 28/28 keys, 35 original rows | Native source/oracle check |
| Runtime integer production and cyclic union | Exact key-associated original graph | Native source/oracle check |

Four comparisons mean SK vs SK+MK separately on both backends, plus pristine vs
GPU separately for each source. Marker-effect comparisons project out only the
new marker relation; cross-backend comparisons include it. No incomplete
snapshot is accepted as equal. The SK-only source is the control, not the
allocation-sensitive original source.

14 focused Rust tests (GPU worker enabled) and 16 Python tests pass. The latter
include complete source transformations, late mutable values, finite Run,
union-created bindings, subsumption exclusions, strings and rejection behavior.
The numeric key oracle compares each original row's attached key and its public
value/equality/visibility, not just the count or histogram of keys.

For the real `add` fixture the marker is selected for consumers that do not
produce a runtime marker tuple in this input. That fixture validates conversion,
not a marker benefit. The separate active fixture prevents an all-empty marker
comparison from being mistaken for a working publisher/consumer path.

Focused regression commands, after building the examples and native test runner:

```sh
cargo build --offline --release --target-dir target/analyzer \
  --example structural_key_source --example standalone_key_verify --example lhs_ast
cargo build --offline --release --example native_math_baseline --example lhs_ast
CELL_MATRIX_WORKER="$PWD/out/worker/matrix" cargo test --offline \
  --manifest-path cell-assisted-egglog/Cargo.toml --target-dir target/key-tests \
  --test program_modes --test stable_choice --test full_structural_keys
/path/to/solver/python -m unittest \
  tools.cell_spine.test_structural_keys tools.cell_spine.test_pass3_markers
```
