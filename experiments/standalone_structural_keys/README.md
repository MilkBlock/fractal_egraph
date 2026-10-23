# Automatically generated shared DAG structural keys

The compiler now reuses **automatic-refutation's native `Arc<Node>` keys**. It
no longer expands a key into a BigRat or BigInt. The generated `.egg` explicitly
contains its native-extension declaration, key functions, maintenance rules,
input freezing, selection policy and schedule. The runner only registers the
named primitives; it does not silently rewrite rules or add schedules.

The user-approved acceptance contract is:

```
source.egg -> source_sk.egg       may change selection/results
source_sk.egg -> source_sk_mk.egg must preserve the SK result
```

SK is the marker oracle. Neither the original source nor the earlier full Rust
selection policy is an equivalence gate.

## Shared representation and automatic generation

`stable_choice/key.rs` is the same implementation for the main runtime and the
pristine-kernel extension runner:

```
Key = Arc<Node>
Node = { anchor, height, typed label, child Key references, cached hash }
```

Children are referenced, not copied. The cached hash accelerates lookup;
collisions still require exact structural equality. Order uses input anchors,
height, typed labels, and child order. A same-pointer comparison finishes
immediately. No allocation ID or hash-only tie-break is introduced.

From the native parser and schema the compiler automatically:

1. Finds the selection roots and their transitive constructor input sorts.
2. Emits each `CellStructuralKey-Sort` function and its constructor-key rules.
3. Freezes input keys once after an empty-LHS one-shot initializer, if present.
4. Finds rules that write `to-hoist` and schedules maintenance before those
   rulesets, including combined rulesets containing them.

The default `--key-policy automatic` preserves the main thread's **to-hoist-only**
policy: prefer greater `Expr-size`, then the smaller structural key. Other
selection sites are unchanged. `--key-policy full` is explicit and additionally
rewrites the known Smaller/IVT/equal-cost-improvement shapes; it requires more
maintenance and has a separate validation result.

For raytrace Pass 3, automatic mode selects **107 of 242 constructors**, with
maintenance before **boundary-analysis**, not before every Run.

A generated fragment looks like:

```egg
; cell-structural-key-runtime/v1 {"storage":"shared-native-dag",...}
(function CellStructuralKey-Expr (Expr) CellStructuralKey
  :merge (cell-key-min old new) :unextractable)
(rule ((= e (Pair a b))
       (= ka (CellStructuralKey-Expr a))
       (= kb (CellStructuralKey-Expr b)))
      ((set (CellStructuralKey-Expr e) (cell-key-node-Pair ka kb)))
      :ruleset cell-stable-keys)
```

The header shown above is abbreviated; generated files contain the complete
primitive schema. The key type is registered by that explicit extension.

## Execution boundary

**This is not a bare-pristine standalone program.** It requires the declared
native DAG extension, as agreed with the user. `program-modes` builds the same
extension against the immutable `egglog-baseline` kernel and against the current
runtime. The kernel is unchanged. Reports include `setup.runtime_extension`.
In GPU mode key maintenance runs natively; original admitted queries remain on
GPU. Both backends consume the same source. Use `program-modes` for these
files; do not send an already-generated SK program through a legacy runner
that installs/reapplies its own hidden stable-choice policy.

Snapshots also retain sharing: `structural_key_dag` is one topologically ordered
node array, with key-valued rows referring into it. The comparison harness
interns **exact node descriptions** across the two snapshots before comparing
facts. It does not compare per-process node IDs or hashes. A 30-level repeated
Pair test keeps 30 Pair nodes rather than expanding over a billion tree nodes.

## Commands

Use the versioned benchmark skill (Python 3.9+ and the existing solver Python):

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py structural-pipeline \
  --source /path/to/source.egg \
  --analysis experiments/analyze_library_credit/library \
  --output out/dag-source-new --worker out/worker/matrix
```

This outputs `source.egg`, `source_sk.egg`, `source_sk_mk.egg`, manifests and logs.
Add `--check` for the four source/backend checks. **A generated source is not a
passed check:** a mismatch remains a nonzero exit status with retained evidence.
Use `--key-policy full` only when that policy is desired. `--no-build` reuses
verified binaries; `--solver-python` selects an existing solver installation.

Run a generated source through the existing interface:

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py program-modes \
  --source out/dag-source-new/source_sk.egg \
  --marked out/dag-source-new/source_sk_mk.egg \
  --native-rulesets cell-stable-keys,cell-stable-freeze \
  --worker out/worker/matrix --output out/dag-check-new
```

The source marker stage remains the existing **two-atom, fully-keyed positive
view adapter**. This change does not enlarge its design space or claim a new
Cell closure proof. It preserves the explicit extension header verbatim.

## Verification and limits

`RESULTS-DAG.json` records the new measurements separately from the historical
BigRat `RESULTS.json`. The latter is obsolete as a current performance claim.
The new representation runs full raytrace, but its source-marker comparisons
must be read separately; unchanged selection sites can still expose execution
order differences. Failed comparisons are not hidden by excluding tables.

The compiler still requires a flat program with declarations/rules before
inference. Unsupported container fields in relevant constructors fail on actual
instantiation. Native literal primitives support computed String/f64 values;
the old finite literal-pool restriction has been removed. All input checks stay
in the generated program and can fail when they assumed the original selection
policy.

Focused tests:

```sh
cargo build --offline --release --target-dir target/analyzer \
  --example structural_key_source --example lhs_ast
cargo build --offline --release --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/analyzer --bin cell-assisted-egglog
/path/to/solver/python -m unittest \
  tools.cell_spine.test_structural_keys tools.cell_spine.test_pass3_markers
CELL_MATRIX_WORKER="$PWD/out/worker/matrix" cargo test --offline \
  --manifest-path cell-assisted-egglog/Cargo.toml --target-dir target/key-tests \
  --test program_modes --test stable_choice --test full_structural_keys
```

## Final full-raytrace result

| Variant | Inference seconds | SK vs SK+marker |
|---|---:|---|
| Baseline kernel + DAG extension, SK | 23.968183 | equal |
| Baseline kernel + DAG extension, SK+marker | 23.560746 | equal |
| GPU queries + same native extension, SK | 18.136369 | equal |
| GPU queries + same native extension, SK+marker | 20.853203 | equal |

Preparation and GPU compilation are separate in `RESULTS-DAG.json`. Original
unmodified-source pristine inference was 22.574276 s in the earlier measured
control: the new SK native inference adds about 6.2%. These are single
observations, not averages. They do not establish marker acceleration: this
raytrace library selects panic-check consumers and produces no marker rows.

The first source-marker adapter inserted completion before every Run. Its native
result differed. Completion now runs only before actual consumers (and combined
sets containing them); the exact SK/marked comparison passes on each backend,
without excluding Smaller or any other original SK table. The old failed reports
remain in `out/dag-raytrace-pristine` and `out/dag-raytrace-full-pristine`.

**Cross-backend equivalence is still failing** for the same source, including
Smaller/TCPair/TermConcat differences. `program-modes` retains those failures and
its nonzero strict four-way exit status. No new extraction-quality conclusion is
claimed. Final evidence: `out/dag-raytrace-final-check/`. The SK source repeated
in a fresh pristine process is exactly equal to its first run.

The active small publisher test passes all four comparisons and actually
produces a marker consumed by three rules. 18 Python, 14 Rust integration, and
one deliberate hash-collision unit test pass. The obsolete expanded-tree
`standalone_key_verify` oracle was removed; old Rust ordering is not an acceptance
requirement for source generation.
