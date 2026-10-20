# Full structural keys: a small real eggcc case

**Accepted contract (user decision): final selected-program equivalence.**
Full intermediate graph equality remains a diagnostic and is not claimed.
The original, key-enabled and perturbed `fact` executions have identical Tiger
outputs; the actual analyzer output was also compared to the original for
`fact` and the smaller `add` case. See [observable-acceptance.json](observable-acceptance.json).
The opt-in analyzer flag is `--structural-keys`; it is off by default.

Input is the complete `fact` Pass-3 snapshot exported from production eggcc
`ae80f82a`, with all its original declarations, rules and main schedule. This is
one frozen Pass replay on the local egglog fork, not three passes rerun from
Bril and not a GPU benchmark. The source and its checksum are committed here.

## Changes under test

- Generate shared structural keys for all equality sorts produced by constructor
  declarations, including lowered relations; not only those reachable from Expr.
- Preserve the existing input-anchor scheme and exact structural comparison
  after hash equality. No eclass-ID or hash-only tie-breaking.
- Maintain keys before each original Run and once at the end.
- `to-hoist`: maximum size then structural key.
- `Smaller`: minimum cost then Term key; allow a later equal-cost but better-key
  candidate to update `ExtractedExpr`.
- `IVTMin`: minimum length then the whole IVTAnalysisRes key.
- Unknown container fields are never encoded by their allocation IDs. An unused
  declaration is allowed, but an actual unsupported constructor instance raises
  an error. The first diagnostic run exposed `ExprSetPrim`; this sample does not
  instantiate it.

The `policy-rules.egg` generated per lane records selection rewrites. It requires
the existing Rust structural-key primitives and generated helper rules, plus the
maintenance schedule. It is not a standalone pristine-egglog program.

## Single-run observations

| Variant | Main execution | Strict final graph vs full-key reference | Actual Tiger DAG vs full-key reference |
|---|---:|---|---|
| Original policy | 17.62 ms | Different policy; different graph | Not used as the determinism reference |
| Full keys + stable choices | 96.06 ms | Reference | Reference |
| Same configuration, fresh process | 91.26 ms | Pass | Pass |
| Four-thread configuration | 98.83 ms | Pass | Pass |
| Reverse contiguous rule declarations | 110.40 ms | **Fail: Smaller** | Pass |

Each lane runs once. The perturbation runs are correctness checks, not samples
for averaging. Four configured threads do not prove this tiny workload actually
executed parallel work. Main execution includes full key maintenance and the
changed selection policy; it is about 5.45 times the original in this case.
This is not a measurement of hash computation alone. Initialization and exports
are separate; full-key setup was about 134.82 ms.

All **811 final constructor-output eclasses across 36 populated sorts** have a
key. This includes the lowered relation sorts. It does not establish key
availability at every intermediate point or support for every declared container.

Production Tiger selected identical constructor DAGs for `fact` and `main` in
all four full-key variants (61 serialized rows before reconstruction). A separate
strict graph diagnostic finds that excluding `Smaller` makes the reverse-order
comparison pass. This projection is diagnostic only: **Smaller is a real table read by rules.**
The strict graph result stays failed; the user explicitly chose final selected
program equivalence as the acceptance gate for this feature instead.

The difference demonstrates the distinction between choosing a stable winner
and constructing a stable history of intermediate merge nodes. Structural keys
do not automatically canonicalize `(Smaller a b)` versus other merge histories.
We have not implemented normalization of that history, and do not claim a
universal graph-determinism guarantee from successful replay or extraction.
The new acceptance packet preserves that strict failure rather than rewriting
it as a passed full-graph check.

## Reproduce

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py structural-keys \
  --snapshot experiments/structural_keys/fact-pass3.egg \
  --output out/fact-structural-keys-new
```

Use Python 3.9+. The skill builds the example and the existing Tiger output
checker, runs five isolated processes and production Tiger, and saves raw graphs
and exact controls under `out/`. It returns nonzero if execution/key coverage or
actual Tiger selected-DAG equality fails; strict graph differences are retained
as diagnostics. No analysis, trace capture or GPU worker
is started. Read `results.json` here to reuse the recorded result.

Focused tests cover late same-cost choices, IVT ties, extra constructor sorts,
unsupported instantiated containers and the existing stable-hoist behavior.
The ordinary analyzer behavior is unchanged unless `--structural-keys` is used.
This branch is isolated from the main thread worktree.

## Analyzer option

```sh
EGG_LAYOUT_GRAPHS=0 cargo run --offline --release --target-dir target/keys -- \
  analyze --source experiments/structural_keys/add-pass3.egg \
  --output out/analyze-add-keys --structural-keys --save-history
```

`add-pass3.egg` is another complete production snapshot, from eggcc's own
`tests/passing/small/add.bril`; its entire ruleset and schedule are retained.
The normalizer writes `normalized.egg`, records the rewritten rule history and
marks `analysis.json` with policy `eggcc-v1`, acceptance
`selected_program_equivalence`, and `whole_graph_deterministic: false`.
This is a selection policy, not an automatic equivalence proof for arbitrary
inputs. Declarations and rules must precede inference; the profile requires the
eggcc `to-hoist` schema. Supported examples are checked separately with production Tiger.

A verified one-shot empty-LHS initialization is executed before freezing keys
and treated as input construction, not as a giant coarse composition. Other
Run/Saturate boundaries are retained. Key-maintenance matches are excluded from
user rule history. Exact key payloads have a native `cell-key-decode` replay
representation; the policy header travels with history and extracted entries.
Offline history replay restores it automatically; do not pass the flag again.

Local ripen uses the same policy environment. Synthetic RipenInput constructors
are explicitly opaque: keys are not invented from their placeholder names or
slot indexes. Recorded boundary-key evidence remains available. Parent snapshot graft reuse and
symbolic Expr-size lowering are deferred for this policy; concrete preparation
remains available. Planner reuse/transfer export with an unversioned library is
rejected explicitly. Unknown instantiated containers still fail rather than
using allocation IDs as structural keys. Full-graph equality checks in other
benchmark modes and local Cell certification are not relaxed by this option.

Final native graph validation passed for `fact` with analyzer tracing enabled.
The full `fact` composition-discovery runs were not completed: large histories
used substantial memory, and one run hit a stack overflow. They are not reported
as successful complete analyses. The smaller real `add` snapshot and a staged
same-cost regression exercise complete analysis, local ripen and history replay.
The optional environment variable `EGG_LAYOUT_KEY_AUDIT_GRAPH` saves the final
native graph for this validation; ordinary analysis does not export it.

## Observability warning and native counterexamples

`analyze --structural-keys` now writes `semantic-audit.json`, embeds it in
`analysis.json`, and emits a warning during capture (also during history replay).
The report treats `Smaller`, `IVTMin`, `to-hoist`, and `ExtractedExpr` as
**potential** differences introduced or exposed by the selection policy. It does
not claim to have measured a difference in this particular run.

It lists their consuming rules, rulesets, effects and transitive table paths.
RHS lookups and function merge expressions are included. A reachable union
conservatively propagates to all tables because rebuild can change keys and
invoke merges. This deliberately broad, schedule-insensitive diagnostic does
not prove that guards can be satisfied, that all paths run, or that an external
extractor observes a difference. Unsupported constructs remain unresolved.
Absence of a reported path is not a safety certificate.

The three different conclusions must not be confused:

| Evidence | Conclusion |
| --- | --- |
| Static dependency path | Potentially observable; warn |
| A native query returns different answers after the same continuation | Concrete counterexample; reject this replacement |
| All queries agree in a tested continuation | No counterexample in that test; arbitrary future reuse remains unproved |

`cell_replacement: not_certified` is explicit. Final-output equivalence is marked
`not_checked_by_this_run`; running the analyzer does not itself rerun production
Tiger. Existing exact local Cell equivalence gates remain unchanged.

Run the small native counterexample:

```sh
target/keys/release/egg_layout analyze-continuation \
  experiments/structural_keys/observability/left.egg \
  experiments/structural_keys/observability/right.egg \
  experiments/structural_keys/observability/continuation.egg
```

Both states select `(TCPair (Leaf 1) 2)`. After an identical later `observe`
ruleset, only the left state contains `(SeenLeft (Leaf 1))`, and only the right
contains `(SeenLeft (Leaf 2))`. This is an executable model of the risk, **not a
claim that eggcc contains this extra observer**. Static auditing of the real
`add-pass3.egg` separately finds the existing Smaller-reading union rules.

The command prints JSON and exits nonzero on a counterexample or an inconclusive
audit. `check` commands are probes on clones, so false queries do not stop later
observations or change either state. Actual execution/type errors are unresolved,
not interpreted as false facts. Schedules and intervening actions retain their
original order. No extra saturation is inserted. A successful exit only covers
the supplied queries. Execution is explicit and can run as long as the supplied
schedule; this diagnostic is never started automatically by normal analysis.

The CLI accepts plain flat native replay sources. The Rust `compare_states` API
also accepts already prepared engines, including structural-key environments,
with their exact continuation and key-maintenance schedule supplied by the
caller. It does not reconstruct engines from serialized graph dumps. This is
actual native egglog execution, not a symbolic simulation or GPU benchmark.
