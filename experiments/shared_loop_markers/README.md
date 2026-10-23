# Solver-selected shared loop-context markers

Actual native/shared CPU-GPU runtime, using two unmodified rules from the saved
raytrace Pass-3 program. This is exact query-prefix reuse, not a new saturation
proof. No `DoWhile` names or 84-instance assumptions occur in the implementation.

The first consumer propagates the input context to a loop. The second constructs
loop-invariant constants. Their common prefix is `DoWhile + ContextOf(inputs)`;
the second consumer continues through output context/type and finally `Const`.

```text
DoWhile
   ↓
input ContextOf ────────────────→ original context-propagation RHS
   ↓
output ContextOf
   ↓
HasArgType ──→ remaining Const delta query ──→ original lsr-inv RHS
```

## Implementation

- `tools/cell_spine/shared_prefix.py`: alpha-normalized typed prefix keys,
  explicit cost inputs and inspectable `.egg` marker definitions. Variable
  aliases, argument positions, constants and sorts are retained.
- `tools/cell_spine/scheduled.py`: the existing SCIP/Gurobi interface chooses
  anchor/payload/private-or-shared plans jointly. A shared prefix and its parent
  chain are charged once across consumers. It explores the exported fixed
  read orders and prefix/layout domain, not all possible join orders/subgraphs.
- `cell-assisted-egglog/src/parameterized_cell/shared_marker.rs`: one correlated
  prefix relation per chosen definition. A child extends its parent's answers.
  Native row commits invalidate only affected roots and descendants; consumers
  refresh them lazily. Missing-port subscriptions, removals, function updates,
  union/rebuild and branching answers are retained. Consumer watermarks/guards
  remain independent. Original RHS, Run boundaries and saturation progress do
  not change. The new module does not change the egglog kernel.
- Markers register as `typed-shared-prefix/v1` query certificates in the common
  Book. They cannot authorize sat-skip.

`program.egg` is unchanged. `boundary-programs.json` is the checked executable
plan; `shared-markers.egg` states the generated relations/rules for inspection.
The helper rules are NOT scheduled as extra table scans. Runtime maintenance
comes from the native commit feed. `spines.egg` retains the selected layouts.

A prefix stores correlated alternative bindings, not independent per-column
sets. Selected payload fields permit direct restoration; omitted fields retain
parent/row receipts. No assumption of a unique context/type is made. Prefix
cache entries do not contain consumer-specific timestamp-filtered answers.

## Measured input to the solver

`probe.json` selects two real source queries and the existing DoWhile anchor for
a diagnostic pilot. `CELL_SHARED_MARKER_PROFILE=1` materializes all their prefixes
for this measurement only. This can be expensive for large branching prefixes;
it is opt-in, not the production layout or a hidden search budget.

The pilot found:

| Prefix | Peak correlated bindings |
|---|---:|
| DoWhile | 84 |
| + input ContextOf | 84 |
| + output ContextOf | 84 |
| + HasArgType | 84 |
| + Const (complete match) | 122,846 |

The solver considered **690 layouts** and selected the two-atom marker for the
first consumer and the four-atom marker for the second. Their parent chain is
shared. The complete-match Const marker was not selected. The selected marker
DAG has four definitions and 84 live bindings per definition.

Pricing includes shared maintenance once, payload/receipt storage, remaining
reads and the existing private cache's required-slot reconstruction. A supplied
`--marker-profile` contributes observed binding peaks and edge refresh work.
Unmeasured alternatives retain explicitly labeled structural estimates; empty
setup tables do not prove that future cardinality is zero. This is optimal only
for that cost model and offered domain, not a proof of fastest wall time.

## Validation and measurements

- Two alpha-renamed consumers with different guards and separate rulesets agree
  with native incremental queries after late facts, min updates, multiple
  contexts, union, deletion, subsume and reinsertion. Omitted/all payload variants
  are both exercised. Unrelated rows do not refresh these markers.
- Rooted-query, parameterized, scalar, closure and artifact regressions pass.
- Full Pass 3: same-submit CPU/GPU original facts, values, equality and visibility
  agree; production Tiger selected DAG agrees (29 functions).
- Native Cell-query audit: **34,162 checks passed**.
- The preexisting fused-native mismatch remains. No pristine parity is claimed.

Same binary, same selected plans/payloads, each lane run once:

| Measurement | Private caches | Shared prefix DAG |
|---|---:|---:|
| Complete inference | 14.390996 s | 14.319296 s |
| All Cell-query handling (not just these two queries) | 0.882959 s | 0.894603 s |
| Boundary port/index reads | 1,484 | 1,346 |
| Additional shared seed-row reads | 0 | 84 |
| Non-seed CPU row visits, including shared work once | 123,617 | 123,506 |
| Remaining GPU candidate-row visits | 10,393,521 | 10,393,521 |

The total-time difference is **0.50% in one pair**, while aggregate Cell-query
handling did not improve. This does **not establish a substantial or stable
speedup**. The useful result is that source-selected consumers now share actual
state and prefix work. Both controls already have loop-query takeover; its old
6.95% benefit must not be credited to this new sharing mechanism.

GPU Join-depth histograms and the fixed-rule-weight survival curves are identical
for the 310 common active queries. Sharing moves no further query off the GPU.
The DAG stores 336 derived prefix frames / 2,016 payload scalar slots and 252
shared dependency edges; these are not RSS measurements. Cache-hit counts include
repeated access by the same consumer and are not independent Cell credit.

## Reproduction through the existing bench skill

Use Python 3.9+ and the configured solver Python. Reuse unchanged CPU controls.

1. Diagnostic pilot: `CELL_SHARED_MARKER_PROFILE=1 CELL_SEARCH_COUNTERS=1` with
   `bench.py run --all-rules --joint-sat --boundary-programs probe.json`.
2. `bench.py spine-program --query-selection queries.json --table-profile
   PILOT/report.json --marker-profile PILOT/report.json --compile-only`.
   The compiler checks profile source provenance. All constructor anchors remain
   solver choices; the pilot's root is not a selection constraint.
3. Run the generated `boundary-programs.json` with `CELL_SHARED_MARKERS=0` and
   `=1`; use `--all-rules --joint-sat --reuse-controls CONTROL` for both.
4. `bench.py shared-marker-credit --before PRIVATE --after SHARED` compares
   identical plans. Add `--profile-before/--profile-after` from separate counter
   runs for complete per-query/depth metrics. `cell-ledger` includes shared
   prefix rows once. Use `audit --cell-only` and normal production extraction.

Saved artifacts: `out/shared-loop-selected/program`, `out/shared-loop-private`,
`out/shared-loop-reuse`, both `*-profile` directories, `out/shared-loop-audit`,
and `out/shared-loop-checked-credit.json`. Relevant compact results and generated
marker semantics are preserved alongside this README.
