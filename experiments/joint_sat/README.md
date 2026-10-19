# Guarded joint terms saturation skip

**Full Pass-3 follow-up:** [the complete benchmark](../raytrace_pass3_gpu/JOINT_PASS3.md)
finds zero macro admissions: all 12 boundaries hit the global removal-history
guard. GPU time rises from 19.025 s to 28.111 s with the observer enabled. The
initialization-only successes below therefore do not establish a Pass-3 benefit.

This completes an **executable, guarded joint transfer** for
`terms`, `terms-helpers`, and `terms-helpers-helpers`. It runs in the same
native engine and shared CPU/GPU event loop. New covered Cells publish the
joint result through native macro actions; the original smooth queries and
their RHS executions are omitted. Uncovered states retain ordinary execution.

This is not unconditional saturation for arbitrary rules/merges. The contract
below is checked before materializing effects. It is also not a speed benchmark:
macro evaluation, boundary reads, native commit/rebuild, and proof maintenance
still cost work.

## The joint transfer

The compiler accepts producer rules shaped like the actual eggcc terms rules:

```text
constructor(operator/other parameters, child Expr ports)
    + each child's ExtractedExpr = TCPair(Term, cost)
    -> PotentialExtractedExpr(root, TCPair(Term-expression, cost-expression))
```

The constructor, typed slots, Term expression and checked cost expression come
from the source AST and native compiled query. They are not hardcoded per Bop.
The initialized raytrace fixture compiles all eight producers: Const, Arg, Bop,
Top, Uop, Get, Single and Concat. Costs currently admit i64 literals, operand
costs and checked addition, sufficient for those producers.

The other six rules are a deliberately explicit proof contract in
[`joint_helpers.egg`](../../cell-assisted-egglog/src/parameterized_cell/joint_helpers.egg).
The compiler compares the installed AST bodies/actions, helper count, constructor
roles and `ExtractedExpr :merge (Smaller old new)` with that contract. Altered
guards or merge policies fail compilation. Source variable names in this helper
contract are currently significant; arbitrary alpha-renamed helper definitions
need normalization before this compiler can accept them.

For one newly eligible producer with stable ports, let its computed candidate be
`TCPair(t,c)` and the initializer be `z = TCPair(NoTerm,S)`:

| Condition | Native macro actions |
|---|---|
| `0 <= c < S` | Produce the candidate; set the function to z; set it to the candidate through the original Smaller merge; union `Smaller(z,candidate)` with the candidate |
| `c >= S` | Produce the candidate; initialize the function to z |
| Negative cost / checked overflow | No macro effects; execute the ordinary schedule, including its native error |

The macro retains the **candidate, Term, TCPair, Smaller and function rows**.
It does not replace the function with a hand-written numeric min and throw away
the intermediate graph. The preflight uses `min(c,S)` only to propagate the
known selected cost to a dependent producer before committing any effects.

The pure preflight constructs dependency waves from actual Expr ports. A child
Cell can supply the next Cell's port; there is no new concrete closure catalog
entry for a different Term/cost. Missing external inputs and unseeded cycles
stay blocked. Numeric conditions for all planned waves are checked before the
first macro publishes anything. Equal macro programs in a wave share one native
action batch.

## Why the contract is sufficient, and when it is not

The materializing fast path requires:

1. The exact original joint nested Saturate schedule. Associative Sequence
   spelling is normalized; a different order, `until`, or ordinary Run does not
   acquire this certificate.
2. One producer constructor per active Expr e-class and one visible TCPair per
   relevant result class. Multiple producers or candidate aliases decline.
3. A new result key, or a previously certified stable result used as an input.
   Changed existing function state, external helper work and union invalidate
   extension until the ordinary backend has completed the boundary again.
4. The producer binding is actually eligible under its native semi-naive
   watermark, or is enabled by a newly planned child result. An old consumed
   producer whose output was deleted is not a new Cell.
5. No currently subsumed helper rows. The mirror retains exact invisible keys
   separately from missing physical keys: ordinary rebuild removals no longer
   permanently disable the observer. Hashcons can return an invisible old
   TCPair/Potential/Smaller without restoring visibility, so those rows still
   decline the macro. Already completed, unchanged boundaries can still skip.

Under these conditions each new producer has one candidate. The initializer
establishes S; improvement either does not fire or proposes exactly that one
candidate. The corresponding Smaller comparison selects it. No other candidate
or cost is lost. Dependency waves use only stabilized child outputs, matching
the original outer `terms` / inner helper saturation order. All three original
query frontiers can then be acknowledged together.

This certificate concerns **the actual ordered semi-naive execution state,
including its cursors**. It is not a promise that arbitrary non-semi-naive
reapplication of effectful `set` rules would materialize exactly the same
intermediate facts. Tests exercise finite Runs and deletion after handback to
protect this distinction.

On failure of a boundary guard there is no partial macro publication. The shared
runtime finishes with its normal CPU or GPU matcher. After a successful ordinary
joint saturation, the observer renews its completed-state certificate. A changed
rule environment disables the hook. Errors are never certified as completion.

## Shared runtime integration

```text
ordinary input / Run
    -> native row and union journals
    -> observer invalidates affected guarantees

exact joint Saturate boundary
    -> validate contract and producer port dependencies
    -> native closure macro batches OR normal matcher fallback
    -> publish all consumed journal rows to the GPU mirror
    -> acknowledge all three original rulesets and CPU audit-query cursors

later GPU/native query
    -> sees macro-produced facts; old work is not replayed
```

The observer temporarily borrows the existing engine. It does not clone an
egraph per Cell and does not retain a separate live graph. Runtime row deltas
consumed by the observer are forwarded to the shared matcher in commit order.
The original positive-constructor sat-skip hook remains supported.

The proof compiler is a separate trusted compiler module, not an inferred
generic fractal family or a claim that arbitrary Analyze outputs are executable.
There are no candidate budgets or empirically chosen admission thresholds.

## Reproduce and inspect

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py \
  joint-sat --output out/joint-sat-new
```

To verify the real raytrace producer rules and initialized input:

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py \
  joint-sat --snapshot /Users/mineralsteins/Repos/egg_related/egg_layout/out/raytrace-interactive-20261010/pass-3-flat.egg \
  --output out/joint-sat-raytrace-new
```

The latter keeps the snapshot's native initialization, omits its main Pass-3
schedule, then runs just the terms joint saturation. It is **not a complete
Pass-3 benchmark or a Tiger extraction measurement**. Every reported boundary
must pass full original row/value/equality/visibility comparison with native
execution. The GPU lane also uses its incremental native-query audit on fallback.

The small workload checks parallel roots, a three-deep dependency, further
grafting, a finite Run, an external cost update, union and quiescence. Its first
two phases assert zero original smooth bindings, not merely a positive skip
counter. [`tests/joint_sat.rs`](../../tests/joint_sat.rs) additionally covers
missing ports, the sentinel branch, aliases, wrong schedules, native errors,
subsumed candidate reuse, consumed/deleted outputs, and CPU/GPU handback.

Source: `cell-assisted-egglog/src/parameterized_cell/joint.rs`; integration:
`table_live::SaturationHook`; evidence:
[results/summary.json](results/summary.json). The normal skill `test` entry runs
the new regressions together with existing runtime/sat-skip regressions.

## Costs and remaining boundary

The saved release checks passed on both shared CPU and shared GPU paths:

| Workload/boundary | Native smooth bindings | Joint-path original smooth bindings | Macro instances | Exact whole graph |
|---|---:|---:|---:|---|
| Small workload, first two fresh-Cell saturations | 75 | 0 | 15 | yes |
| Initialized raytrace input, terms saturation only | 16,351 | 0 | 4,077 | yes |

The raytrace result has 27,792 original rows and uses eight producer templates;
132 dependency waves are committed. Its complete 14-rule terms subsystem is
installed. The small workload subsequently exercises three fallback boundaries
and ends at 244 native versus 169 joint-path original smooth bindings; the
75 skipped bindings are not confused with these later ordinary executions.
An additional quiescent skip is counted separately from materializing macros.

See [the small-workload report](results/summary.json),
[the raytrace-input report](results/raytrace-input.json), and
[its generated macro programs](results/raytrace-input-macros.egg).
The shared GPU lane still constructs/validates the closure proof on CPU; GPU
matching handles later queries and fallback. The 70 related Rust tests and 10
skill-script tests passed. No timing/extraction speedup is claimed from these
correctness runs.

There are eight fixed producer programs for the real terms family, plus typed
macro variants, not one compiled Cell per numerical value. Actual tables,
instance records and dependencies still grow with data. Current preflight scans
the source rows to form waves; it can cost O(Vd) on a depth-d chain. This is not
yet the optimized event-only admission index. Macro RHS and native rebuild work
remain. No storage reduction or end-to-end speedup is inferred from skipped
logical matches.

Multiple candidates for the same Expr and updates to an existing selected Term
still use the ordinary matcher. The implemented joint proof avoids exponential
enumeration by retaining that explicit boundary; it does not certify every
possible alias/merge interaction as a reusable macro.

### Optimization follow-up: current visibility and scoped callbacks

The next iteration distinguishes actual hidden rows from removed canonical keys,
validates the first entry against current state, skips callbacks for unrelated
Saturate bodies, and stops maintaining unused matching subscriptions in the
observer. A same-run complete Pass-3 control measured 18.663 s with joint off
and 19.714 s with joint on (strict public graph checks passed). This removes most
of the previous overhead but does **not** meet the acceleration objective.

There were still zero macro instances: one boundary declined multiple producers
for one Expr, and eleven declined changed existing state/external helper work.
The benchmark now records existing native per-rule timers without adding hot
per-row instrumentation. `Expr-size` propagation accounts for much of the
remaining always-run work; the Get transfer alone has 956,219 logical matches.
Current same-run evidence is in
`out/raytrace-joint-current-visibility/report.json`. This is an intermediate
optimization measurement; Tiger extraction was not rerun for this iteration.
