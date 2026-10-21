# Committed applications publish and extend runtime spine markers

This connects the typed Analyze spine projections to the actual native commit
and persistent GPU runtime. It is opt-in with `--commit-markers` in the bench
skill (`CELL_COMMIT_MARKERS=1` in the runtime), using an existing Analyze catalog.
It is not another offline matching-only optimization or a claimed speedup.

## Execution contract

1. The compiler matches each projection's ordered, typed atom shapes to fixed
   LHS/RHS row positions in a producer's resolved native actions. Repeated table
   occurrences remain explicit alternatives, with correlated slot equality and
   literal checks. Producers with no usable two-row prefix do not collect data.
2. The original action sink records its already available LHS/RHS values in an
   optional post-action callback. There is no second native query, diagnostic
   TraceSession, or persisted application history. These are observations,
   **not** facts and not evidence that a mutation was inserted.
3. Only after the entire original Run's native action/merge/rebuild commit
   succeeds does the runtime canonicalize those values and resolve them through
   its existing committed-row directory. Invisible or different rows are dropped.
   This rejects stale reads and merge proposals that did not become stored values.
4. Fixed publisher programs align only those already supplied row positions.
   They emit an anchor `(row, generation)` plus a verified prefix of witness-row
   handles. Pending records are coalesced by anchor and plan until upload. This
   is a small binary payload, not a graph export or full history transfer.
5. The GPU checks the current patched rows, generations and complete typed
   binding before installing witnesses. Marker instances are allocated on a
   supplied receipt, not speculatively on every possible anchor. The evaluator
   reads supplied rows directly and publishes the existing ready-port marker.
6. A longer spine keeps the supplied prefix. A genuinely unknown suffix uses
   the old keyed lookup/missing-key subscription. When its row commits, the
   existing exact-key wake carries its row handle, so the prefix is not searched
   again. Union, updates and subsume retain the existing invalidation/fallback.

The marker's state, binding and reverse dependencies remain on the GPU. Native
callbacks and committed row identity resolution are on the CPU. The ledger
reports this CPU work explicitly: zero GPU certification probes does **not**
mean zero directory lookups, copies or host overhead.

Only producers with a relevant structural RHS are tapped. Bootstrap-only or
otherwise uncovered Cells are not assumed absent; their original queries still
run. Guards, ordinary Run's frozen input, native RHS order, and full matching
fallback are unchanged. This does not automatically replay a whole RipSat
closure or skip every internal rule.

## Small native example

`environment.egg` supplies these rules:

```egg
(rule ((= s (Single x))) ((union (Get s 0) x)) :ruleset make-get)
(rule ((= x (Get s 0))) ((Box x)) :ruleset make-box)
```

The Analyze-generated library admits the short `Single -> Get` spine and its
extension through `Box`. `training.egg` was passed to the ordinary analyzer; the
resulting catalog is frozen in `cell-assisted-egglog/fixtures/commit-markers`.
The test executes the native producer and consumers at separate Run boundaries.

| Phase | Old eager certification probes | Receipt-driven probes |
|---|---:|---:|
| Get committed, Box absent | 4 | 1 |
| Box committed, existing prefix extended | 4 | 1 |
| After subsume/union perturbation | 8 | 3 |

The short spine has **zero** certification index probes: its source application
supplied both rows. The one initial remaining probe asks for the not-yet-created
Box. Committing Box adds no new certification probe for the longer spine.
The two initial producer applications perform four CPU directory resolutions;
these costs are not hidden or counted as a GPU speedup.

Every test boundary compares all public facts, values, equivalences and visibility
with native same-submit execution. A native query oracle also checks assisted
bindings. Additional regressions cover: a receipt invalidated by subsume before
upload, min merge retaining 3 instead of proposed 20, failure before publication,
and duplicate/no-op effects that confirm a spine without extending saturation.

## Full raytrace Pass 3 verification

The same five-Cell If/Single Analyze library used in the previous experiment was
loaded into the actual full Pass 3, with original schedules, all 547 rules,
existing joint proofs and native effects. A fresh instrumented run passes the
saved same-submit CPU complete-graph check. The existing fused-native mismatch
is unchanged; no new pristine or Tiger extraction claim is made.

The final compiler selects **3 producer programs / 12 publisher recipes**.
They capture **4 applications / 12 row observations**, resolve 12 row identities,
and emit **4 marker hints**. The runtime retains four marker instances and does
zero GPU certification index probes for this library. The fixed interfaces still
have substantial shared watch-bucket traffic; this work is not free.

An earlier, deliberately broader receipt collector captured 207,788 applications
and 453,745 rows for the same four hints. Static producer recipes removed that
unnecessary collection. This is not presented as an end-to-end speedup: both
Pass-3 runs use diagnostic counters, and no new uninstrumented timing ablation
was performed. The frozen library still has narrow useful coverage.

The normal bench ledger includes `shared_costs.commit_marker_cpu_work` and the
per-plan receipt counters. `results/pass3-ledger.*` is generated by the existing
skill and preserves the report hash, binary/worker/input fingerprints and source
path. `results/on.json` and `off.json` retain the actual small native test stages.

## Reproduce

```sh
# Complete runtime; normal bench setup builds the matching worker automatically.
python3 skills/cell-assisted-egglog-bench/scripts/bench.py run \
  --all-rules --joint-sat --analysis experiments/analyze_library_credit/library \
  --commit-markers --output out/new-commit-markers

# Focused correctness checks (set CELL_MATRIX_WORKER to the current worker).
CELL_COMMIT_MARKERS=1 cargo test --offline \
  --manifest-path cell-assisted-egglog/Cargo.toml --target-dir target/analyzer \
  --test commit_markers -- --test-threads=1
```

Validation also passes 44 Rust tests (including the shared graph checker) and
48 bench/ledger tests. The ordinary non-receipt focused control passes too.

Worker protocol is layout 18; stale workers reject installation. Ordinary paths
without this flag do not install receipt callbacks. The immutable upstream tag
is preserved; the native receipt API is a separate version-controlled change.
