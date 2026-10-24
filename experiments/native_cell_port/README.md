# Complete source-pipeline port to native egglog

**The complete implementation is ported and executes in official eggcc.** The
subsequent stable-selection/export repair passes the final-result gate; see
[the repair results](STABLE_RESULTS.md). An optimized solver-selected native
speedup is still unverified. The original-policy failure below remains as history.

## What was copied

The local `cell-native-egglog` fork contains `cell-extension/framework/`, copied
from research commit `74e7d9b`: the complete Rust analyzer, Cell runtime, Python
structural-key and marker compilers, solver integration, tests and research
fixtures. The imported production Rust and Python algorithms are unchanged. The two
framework Cargo manifests relocate paths and isolate workspaces. The diagnostic
graph checker now uses a dependency worklist and exports exact ID mappings. Every
imported source has a SHA-256 entry in `cell-extension/import-manifest.json`.

This is the existing pipeline, not the abandoned selection-only prototype:

```
official eggcc source
    -> existing native resolver / macro expansion
    -> source.egg
    -> existing structural_keys compiler
    -> source_sk.egg
    -> existing native profile + solver + selected_source compiler
    -> source_sk_mk.egg
    -> existing source_cells::Document and parameterized Cell observer
    -> native Free Join residual queries + native RHS/merge/rebuild
    -> original production Tiger
```

The compiler and runtime read their preceding source artifact. Query wiring and
closure declarations are embedded in the final file. No GPU worker is launched,
and the native experiment does not use the GPU runner's sorted-submission CPU
reference as its residual execution engine. It uses the existing
`step_cell_mixed_candidates` bridge to mix supplied bindings with native rules.

The official eggcc pass driver and Tiger source are unchanged. Its dependency
is path-patched to the local experimental wrapper, which registers the extension
once. Only Pass 3 receives the compiled artifact in this experiment; Passes 1
and 2 remain native and feed their actual Tiger outputs to the next pass.

## Integration size and provenance

| Component | Change |
|---|---|
| New core entry | One dispatch line in `parse_and_run_program`, plus a module declaration |
| Independent adapter | Source loading, existing-runtime attachment, native scheduling and diagnostics |
| Existing kernel prerequisites | Commit `e0891f0`: 50 files, 8,814 added / 245 removed lines relative to upstream |
| Official eggcc driver / Tiger | No source changes |
| Experimental wrapper | One registration line and local dependency paths |

The prerequisite changes include previously implemented Cell APIs, event feeds,
tracing, mixed native batches and kernel fixes. **This is not one total changed
line relative to pristine upstream.** Calling the whole migration a one-line
implementation would be incorrect.

Pinned origins are core `ebba7bb902bdc1b0f377b6bb22c06ac305912674`, experimental
wrapper `08771f9f69de51cc8cdc58e3760b7f00a03e40d8`, and official eggcc `ae80f82a`.
The pristine `egglog-baseline` tag in the research repository is untouched.

## Validation

The automatically compiled counter example runs all three files in independent
processes. Original facts, values, equivalence and visibility agree: **23 rows
and 20 mapped classes**. Its checks cover finite Run boundaries, function updates,
late matches and union. Six adapter regressions pass, including multi-candidate
closure updates, rejected source tampering, wildcard round-tripping and isolation
of the experimental schedule parser.

The real raytrace path runs all three official passes and production Tiger.
The raw native fork and the SK lane produce the same final printed program as
the earlier pristine-core run. The full marker lane also completes, and an
independent diagnostic checks **264,339 Cell query frontiers** against native
complete binding sets. Audit and uninstrumented marker runs produce the same
Tiger output.

However, that output differs from the native SK result. Full original-table
comparison also fails:

| Table | Native SK | Full Cell lane |
|---|---:|---:|
| Smaller | 72,198 | 73,373 |
| TCPair | 63,051 | 63,038 |
| TermConcat | 3,620 | 3,607 |

This is an explicit failed acceptance gate, not proof that the generated program
is semantically wrong. Semantic equivalence of the different extracted programs
has not been established by this experiment.

## Timing observations, not an accepted speedup

Single runs on the same machine, actual official raytrace execution. Pass-3
times include native setup and Cell attachment, and exclude source parsing,
diagnostic export and Tiger. Offline compilation is separate.

| Pass-3 lane | Seconds | Same Tiger output as native SK |
|---|---:|---|
| Raw source, earlier prerequisite-control run | 21.51 | Yes |
| Native source_sk | 20.80 | Yes |
| Full imported Cells | 9.73 | **No** |
| Query reuse, joint closure disabled | 17.57 | **No** |
| Query reuse, direct order-sensitive writers retained native | 17.63 | **No** |
| Attach Cell library, execute all original native queries/schedules | 21.91 | Yes |

The full lane used 327 complete-query declarations and 31 closure programs.
Twelve joint boundaries were handled by the imported machinery. Its diagnostics
show **zero fresh whole-DAG macro instances**; this is not twelve constant-time
replays of an entire saturated graph. It includes the existing parameterized
helper/producer dataflow and comparison closure.

The observed reduction from 20.80 to 9.73 seconds is **not an approved speedup**.
The diagnostic runs are not timing baselines. Per-lane executable copies were
not frozen during this exploratory sequence, so these records must not be
treated as an approved reusable benchmark cache.

## What the isolation established

* Merely loading the complete library and observing native events preserves the
  native extracted result. Parser integration and library attachment alone do
  not explain the difference.
* Replacing native queries with complete binding sets changes the result even
  when joint closure is disabled. The issue is therefore not solely sat-skip.
* Returning direct noncommutative function writers to native queries is
  insufficient. That source-derived safeguard does not prove preservation of
  all later order-sensitive effects.
* The successful audits establish binding-set equality at the observed
  frontiers. They do **not** establish native enumeration/commit-order equality.
  The focused capture below now identifies a concrete native merge-order divergence.

The input has no explicit stable-choice declaration, so the key stage adds zero
keys. It is not valid to claim that the existing pipeline automatically supplies
a stable choice policy for this input.

## Reproduction and artifacts

The fork's `cell-extension/README.md`, `pipeline.py` and `verify.py` are the
entrypoints. `pipeline.py` calls the imported implementations directly and can
install a completed compilation into a recorded native cache entry. `verify.py`
uses the existing bench skill's exact graph comparator.

The Pass-3 compiler reused an already complete native profile for the exact
archived empty-key SK artifact. The newly generated empty-key header differed
only in explanatory `key_semantics` prose; the adapter verifies that there are
no keys/choices/helpers, that all executable bytes and remaining metadata agree,
then reuses the archived SK verbatim. The imported compiler still validates the
exact SK and profile hashes. No incomplete profile or GPU pricing was used.
An unnecessary fresh profiling attempt is preserved as incomplete and excluded.

See [results.json](results.json) for source hashes, all timing records, audit
counts, failed comparison, and local artifact paths. The generated three-stage
files are under `cell-native-egglog/out/native-raytrace-canonical-v2/`.

The prior selection-only experiment is retained locally under the research
worktree's ignored `out/native-port-abandoned/`; it is not part of this port.

## Concrete divergence found with a narrow capture

A cheap per-Run table-count scan finds the first cardinality difference at Run
2160 (`terms`). Checking only the preceding window identifies Run 2154
(`terms-helpers`): its pre-states have **305,549 exactly corresponding rows**,
and both sides produce **377 guard-surviving bindings** from the same binding
multiset. The enumeration order differs.

The native source declares `ExtractedExpr :merge (Smaller old new)` and chooses
the left TCPair when costs tie. Two different TermConcat candidates A and B both
cost 3. Their actual after-state function rows are:

```
native: Smaller(Smaller(initial, A), B)
Cell:   Smaller(Smaller(initial, B), A)
```

Both independently traced native replays reproduce the corresponding actual
after-state, including all typed rows and values. This is concrete effect
ordering evidence, rather than interpreting every trace event as a mutation.
There are 597 logical trace events, including guard-rejected lanes; only 377
satisfy the cost guard. The source's left-biased tie subsequently selects A or B.

Importantly, **this writer is already native in both lanes**. Earlier Cell query
replacement can alter candidate allocation/table order while preserving the
logical graph. Reverting only direct noncommutative writers therefore cannot
restore original behavior. A native-only minimal regression verifies that equal
candidate graphs created in different orders can select different tied winners.
All **12** adapter/diagnostic tests pass.

Stable tie-breaking could repair winner independence, but alone cannot make the
intermediate Smaller trees identical. Preserving the current full-graph gate
requires preserving relevant merge order, or a more conservative upstream
fallback. Changing the selection/observation contract must be explicit. No fix
or native speedup is claimed by this diagnosis.

The exact window covers Runs 2100–2154, not every preceding Run. It is a concrete
located divergence, not a claim of globally earliest semantic divergence.
Reproduction and compact witness: `cell-native-egglog/cell-extension/diagnostics/`.
