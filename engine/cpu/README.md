# Native CPU multi-stage Cell engine

This is an execution target on the repository's **native egglog CPU engine**.
It does not run Metal, interpret GPU instructions, replay terminal snapshots, or
maintain a second fact store. The current target is intentionally opt-in.

The newer `--execution staged` path adds local references, ordered merge
expressions, actual function replies and source-stage-gated publication. It
supports finite repeat without early publication. It still uses complete native
queries to authorize patches; see [the staged execution contract](STAGED.md).
The eager saturation implementation described below remains the default/reference.

From the repository root:

```sh
python3 -m eggfuse cpu \
  --source experiments/cpu_multistage/chain.egg \
  --output out/cpu-chain --check
```

This builds offline, exports typed source rules, runs the existing analyzer's
region/recipe passes, emits `cpu-program.json`, and executes it in a fresh native
process. `--check` runs the same binary in ordinary native mode and compares all
exportable table rows, function values, equality classes and subsumption flags.
It refuses to claim equality if an opaque value cannot be exported. Compilation,
bootstrap, inference and other source commands have separate timing fields.

The Rust executable is also available directly:

```sh
cargo cpu --source experiments/cpu_multistage/chain.egg --prepare out/core.json
# The Python command above performs the shared analyzer/lowering step.
cargo cpu --source experiments/cpu_multistage/chain.egg --plan out/cpu-chain/cpu-program.json \
  --output out/cpu-loaded --mode cell
cargo cpu --source experiments/cpu_multistage/chain.egg --output out/cpu-native --mode native
```

`--plan` may be reused with new input facts, provided the typed rules, schemas and
schedule are unchanged. Native mode does not require a plan. Existing output
directories are rejected rather than overwritten. All declarations/rules must
precede the first schedule in Cell mode; schema mutation and push/pop after that
point require native mode. The library API is `egglog::cpu_cells::CpuCells`.
Compiled objects belong to their compiling EGraph and fixed rule environment.

## Execution

```
typed source + original schedule
    → existing Recipe IR / region batch program
    → CPU target + native source/wiring checks
    → source LHS in native Free Join
    → original RHS + witnessed successor RHSs using the same binding
    → native insert / hashcons / function merge / union / rebuild
```

For `N → P → Q → R` inside a certified joint saturation, the seed query finds N
once. The compiled native action program constructs P, then Q, then R, passing
the actual native values forward. Later scheduled queries can use completion
receipts instead of matching those already completed effects again.

The CPU compiler independently checks every table atom in a successor against
the seed binding and preceding effects. It retains correlated bindings, types,
literals, primitive guards and real constructor identities. An unproved step
remains native residual work; failure of one guard never certifies later effects.
The compiler does not use empirical similarity or a hash as a proof.

Receipts are native tables. A single constructor/Unit effect can use its original
table as an implicit receipt when there is only one source writer. Composite
effects use a private Unit table keyed by the RHS's free input parameters.
Private tables are absent from source exports/extraction. They are reset using
native table generation invalidation, **without executing another Run or moving
the source seminaive clock**. Source and compiled rule variants share watermarks.

Whole-query skipping needs an exact coverage proof: every receipt tuple is a
projection of an original LHS anchor, and equal cardinalities establish complete
coverage. Existing effect rows cannot be assumed to have that origin; source
effect tables only get this count proof when empty at scope entry. Other cases
use native per-binding completion checks.

## Scope and correctness boundaries

- Fusion currently targets positive, jointly saturated regions: constructor and
  Unit facts, exact primitive LHS guards, copies, and eligible constructor union.
- Mixed rulesets are not rejected wholesale. A forward-closed positive component
  can be optimized while independent function updates execute natively. A native
  nonpositive consumer (including later rulesets), destructive writer or opaque
  merge callback blocks early
  publication of the facts it may observe.
- Programs with order-sensitive function merges involving equality/container
  identities currently use the native schedule in full. Moving constructor
  allocation changes native enumeration even for later consumers outside a
  locally certified region; the local proof is insufficient for those programs.
  This includes the current frozen eggcc Pass 3 inputs.
- Function-update RHSs, general numeric RHS programs, containers, subsume/delete
  and uncertified unions continue through native egglog. This version does not
  claim the full GPU recipe language has an optimized CPU lowering.
- A finite Run/repeat remains finite. Saturations nested in a repeat can be
  optimized, but a repeat is never relabeled as saturation. Run-until probes are
  preserved by the native path.
- Scope-entry visibility checks and per-step guards prevent hidden source rows
  from becoming speculative witnesses. Hidden rows combined with union force
  native execution for that activation.
- Auxiliary receipt writes cannot prolong a source saturation. When receipts
  are private, fixed-point detection uses exact source table versions and union
  progress, including updates that do not change row counts.
- This is not a promise of identical internal match order or physical IDs.
  Successful positive closure is the optimization contract. Full-program exact
  endpoint comparison is an additional test, and remains enabled in the suite.
  Erroring programs propagate native errors; speculative partial state after an
  error is not a replay-equivalence guarantee.

Proof instrumentation and raw tracing are currently rejected for compiled Cell
execution. The CLI fixes the native thread count to one for comparisons.
`inline_successor_invocations` counts guarded rule invocations, **not committed
mutations**; repeated constructor/union requests can be no-ops.

## Source map

| Layer | Implementation |
|---|---|
| Shared analysis | `eggfuse/analyzer/regions.py`, `eggfuse/ir/batches.py` |
| CPU artifact | `eggfuse/compiler/targets/cpu.py` |
| Native checker and scoped execution | `egglog/src/cpu_cells.rs` |
| Native cursor / auxiliary-table bridge | `egglog/egglog-bridge/src/rule.rs` |
| Rust CLI | `engine/cpu/src/main.rs` |
| Build, artifact loading, endpoint audit | `eggfuse/engine/cpu.py` |

See [tests and measurements](../../experiments/cpu_multistage/README.md).
