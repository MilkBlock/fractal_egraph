# Complete Pass-3 input for symbolic query-spine compilation

This is an actual complete raytrace Pass-3 run in cell-assisted egglog. The
original `.egg` is kept byte-for-byte, including initialization, main schedule,
RHS, functions/merge and visibility changes. A companion typed plan replaces
selected **queries**, at their original native frontiers. Marker maintenance
is CPU-side and cannot make the original Saturate report progress. Ordinary
Run never becomes a whole-closure shortcut.

## Compiler and execution

`tools.cell_spine.compiler --mode scheduled` (or `auto` for mixed programs) now
accepts the complete input. Native parsing/typing exports all scheduled queries;
combined rulesets are expanded for admission, not rewritten in the program.
The last/main schedule is the optimization region; earlier commands still run
natively. Subsequent commands stay in `program.egg`; the existing Pass-3 benchmark
expects the main schedule to be its last command.

Every connected constructor anchor has a typed query plan. Every prefix goes
through the **existing `markers.layouts`** generator: all nodes/inputs are
possible layout anchors, with anchor/outside/all input payloads. There is no
2-atom limit or requirement for two consumer rules. The outer execution seed
is an actual native row; an inner marker anchor describes carried coordinates,
not a new table scan. The chosen optional fields are actually stored and used
to resume a validated prefix. Missing fields use normal row-handle decoding.
At a multi-row branch only the preceding singleton context is reusable; all
branches remain in the answer.

The scheduled model uses the existing solver backend and layout generator, but
has its own **query-plan objective**, rather than invoking the positive Cell
compiler's whole-closure path master. It selects original vs assisted execution
when a diagnostic profile is supplied, prices shared partial-key indexes once,
and retains one executor per query. The work estimates are not elapsed time or
RSS. Root enumeration retains one structurally compiled read order per root;
it is not enumeration of every Join order, arbitrary payload subset or arbitrary
Cell decomposition. No candidate-count/time budget is applied.

Loading checks original setup, typed slots, atom wiring, guards, anchor, complete
atom coverage and selected fields. It installs no guessed union or cached scalar
truth. Commits/rebuild invalidate read dependencies and copied parameter values;
late facts, aliases, deletions and subsumed roots are handled by the same feed as
the prior boundary service. Native RHS and merge still execute normally.

## What this does not claim

This is **symbolic query reuse**, not concrete checkpoint replay and not a new
RipSat saturation proof. The automatic Analyze catalog has not been expanded.
The scheduled backend currently rejects `--analysis-dir`/`--knowledge-library`
instead of silently pretending to consume their closure certificates. The old
positive-constructor Cell compiler and its library path remain unchanged.
Computed/unsupported predicates and destructive query sinks retain the original
backend; their presence no longer rejects the entire program. Existing joint
terms saturation proofs are unchanged.

`program.egg` alone runs the original program. Optimization needs the auxiliary
runtime to load `boundary-programs.json`; `spines.egg` contains readable layout
annotations, not a standalone optimized egglog implementation. The output reports
`scheduled_symbolic`, `cellified: false`, and never calls itself an archived or
fully cellified program.

## Verified result

Full Pass 3 contains 547 scheduled queries. 461 admit the new structural query
plan format; 31 already have parameterized services. The other 55 retain native
predicate/global/destructive handling. This is compiler admission, **not Cell
coverage**.

The controlled workload is the previously measured loop-context query. Its
selector supplies only the original query name; the old `root: DoWhile` hint is
ignored by the solver. It compares 317 layout/root/prefix options and independently
selects the DoWhile row, a five-atom maximum prefix and optional outside fields.

| Metric | Result |
|---|---:|
| Native source bytes / schedule | unchanged |
| Target complete bindings | 123,254 |
| Live loop instances | 84 |
| Parameter-prefix resumes | 929 |
| Remaining cached row decodes | 27 |
| Fresh port lookups | 1,346 |
| CPU candidate-row examinations | 123,533 |
| Cell-supplied query oracle checks | 32,364, all pass |
| Whole original facts / values / equality / visibility vs same-submit CPU | pass |
| Production Tiger selected DAG vs same-submit CPU | pass |
| Inference, single uninstrumented run | 12.646927 s |

The previous explicit root/port program took 12.455758 s in its historical run.
There is **no demonstrated additional speedup**. Its 126,320 examined CPU rows
fell to 123,533; most of the work is still emitting 123,254 actual bindings.
The pre-existing fused-native graph/extraction difference remains a failing
comparison. Do not report pristine equivalence or use the historical CPU/GPU
ratio as the layout solver's speedup.

An initial invocation used macOS Python 3.8 and failed only while writing the
post-run ledger (`dict | dict`). Inference and graph checks had completed. The
ledger was regenerated with Homebrew Python; no timed run was repeated for this.

## Reproduction

Use the existing skill; this command builds if needed and reuses unchanged CPU
controls. Omit `--no-extract` to run production Tiger as well. Never run timing
alongside builds or another benchmark.

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py spine-program \
  --output out/scheduled-spine-new \
  --query-selection experiments/pass3_boundary_programs/loop-context.json \
  --query-profile out/boundary-depth-off \
  --table-profile experiments/pass3_source_markers/results.json \
  --reuse-controls /path/to/verified/pass3-compact-layout \
  --worker out/commit-marker-worker/matrix --no-extract
```

`--compile-only` stops after source/plan generation. Omitting `--query-selection`
considers all admitted queries; missing/zero measured work is not treated as
profitable. The profile and table-source fingerprints are checked and recorded.
The original hotspot hint is an explicit validation workload, not a hidden top-k
candidate budget.

Use `audit --all-rules --joint-sat --cell-only --boundary-programs FILE` for native
frontier comparison, and `extract RUN --no-build` for Tiger. Fresh-process setup
needs no saved concrete native values from training.
