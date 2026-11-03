# Saturation-local rule continuation

Historical experiment: its old Bake artifact is now intentionally rejected by
the pipeline loader. Use [BAKE_AND_LOCALITY_REPAIR.md](BAKE_AND_LOCALITY_REPAIR.md)
for the repaired pipeline, current reproduction commands and later measurements.

Implemented on `codex/cell-local-saturation`, based on `a677d10`.
This is the **standalone, single-thread Cell CPU runtime**, not native egglog
and not the Metal backend. The feature is **opt-in**:

```sh
CELL_CPU_MERGE_PROGRAMS=1 CELL_CPU_LOCAL_SAT=1 \
  engine/cell-runtime/target/release/eggfuse-cell-runtime \
  --program <authenticated-pipeline-program.json> --output <new-output>
```

## What changed

Within an eligible source `saturate(S)`, a successful rule application can supply
another rule's complete LHS directly from its retained query parameters and
actually visible effects. The existing analyzer's checked relative-binding
routes provide this wiring. It is not inferred from rule names, and it does not
depend on searching the whole graph again.

Execution is:

1. Match the ordinary Run snapshot and execute its original ordered RHS patch.
2. Capture only the coordinates required by verified continuation routes.
3. Drain a deterministic FIFO of directly wired successor applications. Check
   their scalar guards, execute their complete RHS, and feed further successors.
4. Repair the block using the existing global hashcons/merge/union machinery.
5. Keep the ordinary delta queue for unknown boundaries and matches enabled by
   repaired aliases. Finish the original outer saturation control structure.

The runtime retains completion receipts for the active saturation scope. A key
contains every parameter affecting the complete RHS (including fallible
computations), plus the inputs of potentially partial LHS primitives. Once the
key is available at an ordinary query's seed, a completed effect can skip the
remaining query. It does not claim that every skipped LHS witness was a match.

Only successful RHS execution/enqueuing produces a receipt. This is **not** a
claim that every logical application changed the graph, or that queued unions
were already published. Receipts expire on leaving their saturation scope and
are included in checkpoints. Checkpoint restoration checks mode, scope and key
arity; it cannot silently switch scheduling modes.

## Current eligibility and correctness boundaries

- One substantive ruleset per accelerated saturation scope. Nested inert
  marker-publisher scopes may share it. Multiple substantive rulesets are not
  flattened together. Finite `repeat`, finite Run and conditional/probe control
  remain under their original schedule; nested eligible saturations can still
  accelerate independently.
- A route must supply **all** consumer table atoms, with the compiler-validated
  full binding. The verified primitive finish plan checks the remaining guards.
- Constructor/relation creation, scalar computations and eligible unions may
  continue locally. Mutable function reads/updates, hide/delete, panic and
  equality/container-sensitive primitives are not local targets in this version.
- Producer dependencies of ordered or nonmonotone consumers are excluded as
  well. Union influence includes transitive parent-constructor sorts and
  container element sorts, so an indirect collision with an ordered function
  key is not treated as harmless. A mixed ruleset can still accelerate an
  independent safe lane.
- A constructor `set` is a congruence operation: when it merely queues a union,
  its requested row is not yet available as a successor witness. Likewise,
  reusing an already subsumed constructor does not make it matchable. The
  constructor/set paths return actual visibility without an extra table lookup.
- Same-rule reentry is allowed only when the cycle's effect-relevant parameter
  domain is provably nonexpanding. Relevance propagates backward through **all
  continuation wiring, including forwarded query facts**. Otherwise reentry
  returns to ordinary delta matching. This uses no depth/candidate/time budget.
- Global tables, constructor hashcons and repair remain. This is not tableless
  Cell storage, nor completion of arbitrary newly observed RipSat families.
- Known-results/static elimination can conservatively leave a source effect
  unavailable for local forwarding; it then uses the ordinary path. It does not
  invent a physical witness for a reused result.

The sum-bits artifact used here still has the earlier failed RipSat receipts.
This experiment uses its independently checked **typed source continuation
routes**, not a certified shareable RipSat terminal closure. Also, the existing
whole-Pass-3 native `TermBop` discrepancy is not resolved by this work. Whole
Pass-3 acceptance below compares this same compiled input with local scheduling
off/on; it is not a new claim of equivalence to pristine `source.egg` execution.

## Final single-run measurements

All measurements use the same release binary within each pair. Merge programs
are ON; refutation, Sieve, presearch context cache, known-result reuse,
compositions, union-graft and local-congruence experiments are OFF. Diagnostics
run separately. No averages, no concurrent build/test load, no raytrace.

| Workload / metric | Off | On |
| --- | ---: | ---: |
| Synthetic 8-rule chain, 128 inputs: inference | 4.081 ms | 3.219 ms |
| Chain: Run / repair calls | 9 | 2 |
| Chain: exact-key lookups | 2,688 | 1,280 |
| Chain: visited candidate rows | 1,024 | 1,792 |
| Chain: constructor requests | 1,152 | 1,152 |
| sum-bits Pass 3: inference | 139.479 ms | 142.979 ms |
| sum-bits: executor setup | 1.131 ms | 16.362 ms |
| sum-bits: process wall, including parsing/export | 291.498 ms | 311.979 ms |
| sum-bits: Run / repair calls | 1,510 | 1,510 |
| sum-bits: visited candidate rows | 87,218 | 87,218 |
| sum-bits: exact-key lookups | 62,482 | 62,671 |
| sum-bits: constructor requests | 19,199 | 19,199 |
| sum-bits: union requests / effective unions | 3,208 / 1,037 | 3,208 / 1,037 |
| sum-bits: repair dependency visits / collisions | 2,378 / 999 | 2,378 / 999 |

The chain is a mechanism demonstration, **not an eggcc speedup**. Its lower
lookup and barrier counts matter more than this short timing. The larger
candidate count includes reexamined seed rows; completion receipts reject them
before following their remaining joins. It is not a decrease in all row work.

For the real small Pass 3, this final pair is **2.51% slower in inference** and
**7.03% slower in cold process wall time**. An earlier intermediate pair measured
144.15 -> 140.80 ms (2.3% faster); the effect did not persist. Do not promote that
earlier sample into a speedup claim. All intermediate pairs are retained in the
compact results JSON rather than averaged or selectively discarded.

### Why the real benefit is currently small

- Only **137** local successor applications execute: 34 marker publishers and
  103 `terms-helpers-helpers` comparison applications. Longest chain: 3. The 103
  comparison applications are finite same-rule reentries.
- The loaded control does not put most `always-run`, type-analysis and `terms`
  producer/consumer work in one eligible single-ruleset saturation scope. Many
  other routes touch ordered functions, mutable ports or incomplete contexts.
- Those 137 applications do not remove a source Run or reduce candidate visits,
  constructor requests, union requests or repair collisions. Some pure effects
  execute earlier and become redundant later; executed logical applications are
  12,930 -> 12,957, while necessary graph facts remain identical.
- There are 218 captured contexts, 484 transfer attempts, 245 duplicate receipts,
  212 failed guards and 54 unavailable-source-route cases. These are real
  maintenance costs, not free Cell hits. `seed_receipts=0` on this workload.
- The remaining setup includes source-plan eligibility and wiring compilation.
  Sharing union-sort dependency analysis reduced the preceding on-setup sample
  from 100.93 to 16.36 ms, but it remains an additional cold-start cost. Moving
  verified plans into the compiler artifact could remove that runtime work.

Therefore the feature remains OFF by default. The useful next extension is a
consumer that combines preserved contexts from several producers or function
updates, under an explicit merge contract. Merely registering more scopes or
counting more Cell instances would not establish a search saving.

## Validation

- 19 local-saturation tests, including 52 existing semantic fixtures inside one
  test; 9 known-result regression tests; 7 Rust unit tests. All pass.
- Dedicated fixtures compare complete state with native egglog: finite Run
  visibility, inert marker preludes, independent lanes next to ordered merge,
  union-triggered new matches, constructor-set congruence, hidden facts, finite
  permutation cycles and expanding-cycle fallback.
- Forwarded-query regression checks that a rule cannot hide an expanding cycle
  merely because it does not itself use that query field in its RHS.
- Effect-key tests cover different unused witnesses, partial LHS arithmetic,
  and an unused failing RHS calculation. Checkpoint/restart is also verified.
- Separate audit checks every proposed local binding against the actual full
  LHS rows and guards. **282** semantic snapshots for sum-bits (initial state
  included) agree in every row, value, equivalence and visibility flag.
- Snapshot comparison is at outer source Saturate exits and finite Runs outside
  saturation, not internal Runs whose order is deliberately changed. The final
  sum-bits graph has **4,518 records / 2,324 eclasses** in both modes.
- A fresh process with the feature ON reproduces the exact exported state,
  including identities, not merely graph isomorphism.

The audit streams one boundary at a time; it does not retain a large full-run
history in the benchmark's measured process.

## Reproduce

Run from this worktree. The frontend is a native typed frontend, not an ad-hoc
parser. Use a new output directory each time.

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml \
  --bin eggfuse-cell-runtime --example project_stages --example project_semantics

EGGFUSE_TEST_FRONTEND=/Users/mineralsteins/.codex/worktrees/d569/egg_layout/target/debug/examples/eggfuse_frontend \
  /opt/homebrew/bin/python3 -B -m unittest \
  experiments.cell_runtime.local_sat_test experiments.cell_runtime.known_results_test
cargo test --offline --manifest-path engine/cell-runtime/Cargo.toml --lib

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --demo-chain 8 --instances 128 \
  --frontend /Users/mineralsteins/.codex/worktrees/d569/egg_layout/target/debug/examples/eggfuse_frontend \
  --output out/local-sat-chain-repeat --audit

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --compiled out/known-static-compile \
  --output out/local-sat-sum-bits-repeat --audit
```

`--compiled` goes through the authenticated pipeline-artifact loader. It must
reference the existing `source -> source_sk -> source_sk_mk` compiler result,
not a raw-source shortcut. The results record program/binary hashes, environment,
machine, RSS, timing boundaries and the compiler provenance.

Raw outputs are in `out/local-sat-{chain,sum-bits}-final`; the checked-in
`local_saturation_results.json` retains compact measurements and validation
receipts, excluding build outputs and graph dumps.
