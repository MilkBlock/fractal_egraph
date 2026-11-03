# Bake validation and connected marker repair

This repairs the pipeline used by `LOCAL_SATURATION.md`. It is the standalone,
single-thread Cell CPU executor, not native egglog or Metal. All measured lanes
keep `CELL_CPU_MERGE_PROGRAMS=1`; other experimental caches/refutation are off.

## Three fixes

1. **Reject unusable Bake environments.** The old pass checked JSON syntax only:
   all 89 preparations could fail with `Undefined sort CellStructuralKey` while
   the compiler reported success. The native fix already existed; the old
   artifact had not used the rebuilt binary. Compilation now emits and validates
   `bake-health.json`. The pipeline benchmark loader also checks historical
   artifacts without a health file. Environment/type registration failures
   reject the artifact. Replay failures, unfinished observations and unsupported
   closure certificates remain explicitly reported, not promoted into proofs.
2. **Repair completion keys and check them during queries.** Successful pure-effect
   receipts follow representative changes after repair. Generic queries, fixed
   port programs, producer-local projections and merge queries can stop when
   effect parameters identify a completed operation. This is not a negative LHS
   proof: inner short circuits are disabled with the separate refutation/query
   caches, so they cannot contaminate those caches with false empty answers.
   Scalar values and candidate evidence remain intact.
3. **Require structural locality for materialized markers.** The automatic solver
   selected two `Get`s connected only by an integer index, creating 5,668 marker
   rows. Marker fragments now connect through equality-sort variables. For
   disconnected fragments the compiler includes consumer atoms on their shortest
   structural connecting paths. If no path exists, the original query remains.
   Canonical ordering preserves this property in every materialized prefix.

For example, two Read nodes with the same index can include their enclosing
Branch, instead of pairing every Read in the global graph. This uses types,
variables and the consumer atom graph, not Get/If names, sampled cardinalities,
candidate budgets or dropped original matches. Original guards/effects, complete
correlated bindings and exact marker completion remain.

This is a representation policy, not a proof of optimal layout for all workloads.
`--marker-locality all` retains the previous scalar-connected policy for ablation;
`identity` is the default. The unprofiled solver objective is still structural,
not measured runtime cost. Lifecycle profiles remain supported.

## Bake result and remaining limitation

Same 315,811-byte training input, same 50 ms / 200K fact limits:

| Item | Old artifact | Rebuilt artifact |
| --- | ---: | ---: |
| Discovered combinations | 89 | 89 |
| Undefined structural-key sort failures | 89 | 0 |
| Completed saturation observations | 0 | 72 |
| Prepared executable local instances | 0 | 72 |
| Replay rejections | — | 17 |
| Certified reusable endpoints | 0 | **0** |

All 72 endpoint certificates still reject the custom ordered `ExtractedExpr`
merge. Executable programs and observed states are useful evidence, but terminal
Cell substitution is not authorized. **This limitation is not fixed here.**

Training remains narrow: terms (5 Runs), terms-helpers (13),
terms-helpers-helpers (23), and one Run each of cell-key-request-0,
cell-stable-keys, cell-key-ready-check and boundary-analysis. It is not a full
Pass 3 training trace. Only 3 of 36 final markers have witnesses in this input;
13 of 355 offered shapes have witnesses. These counts do not prove search credit.

The execution input is still real sum-bits Pass 3. No raytrace was run. The
public compiler performs source -> source_sk -> Analyze -> source_sk_mk -> typed
recipes -> Cell CPU, validating the training/execution rule environments.

## Single-run measurements

The first successful rebuild reproduced a compiler-layout regression that the
older manually narrowed 47-marker artifact had avoided. This is not a backend
regression or a comparison with pristine egglog.

| Same execution input/runtime, local continuation OFF | Scalar-connected layout | Eclass-connected layout |
| --- | ---: | ---: |
| Markers, including prefixes | 48 | 36 |
| Inference | 353.00 ms | 142.01 ms |
| Process wall including parsing/export | 529.20 ms | 304.38 ms |
| Delta/residual + merge candidate visits | 544,559 | 84,033 |
| Fixed forward port reads | 69,873 | 29,795 |
| Fixed inverse reads | 5,514 | 3,762 |
| Exact-key lookups | 138,082 | 62,212 |
| Constructor requests | 21,771 | 19,188 |
| Repair collisions | 3,242 | 951 |

The dominant benefit is removing oversized auxiliary views. Do not attribute it
to certified RipSat closure skipping. Across layouts, all 3,468 original
source_sk records, values, equivalences and visibility flags compare equal
(2,324 eclasses); only compiler-declared auxiliary tables are projected away.

Same-binary local-continuation ablation on the new layout:

| Metric | OFF | ON |
| --- | ---: | ---: |
| Inference | 142.009 ms | 133.520 ms |
| Setup | 1.193 ms | 16.306 ms |
| Process wall | 304.379 ms | 310.705 ms |
| Run / repair calls | 1,496 | 1,492 |
| Delta/residual + merge candidate visits | 84,033 | 84,032 |
| Fixed forward port reads | 29,795 | 29,807 |
| Exact-key lookups | 62,212 | 62,263 |

One sample: 5.98% less inference time, but 2.08% greater cold process wall time.
There is still no large search saving from local saturation; it remains opt-in.
The historical 139.48 ms run used a different manually narrowed marker program,
not this paired control.

The ON lane executes 144 local successors, repairs 103 receipt keys and uses
103 inner receipts. However, `skipped_remaining_table_atoms=0`: these real-case
hits occur after table reads. Do not call them avoided joins. A dedicated
regression demonstrates skipping inner table reads through both generic and
producer-local paths, but that result is distinct from this Pass 3.

## Verification and cache

- 47 Python tests, 7 native source/marker tests and 7 Rust unit tests pass. The
  Python suites include the existing 52-case semantic suites within tests.
- Native source/marker checks cover the renamed Branch/Read example, late ports,
  union, subsume, finite repeat and effectful functions. Other regressions cover
  repaired receipts, inner Join skips, errors, expanding cycles and checkpoints.
- Separate diagnostics compare 282 semantic boundary snapshots for each layout's
  off/on pair: full facts, values, equivalence and visibility. Fresh processes
  reproduce exact ON identities. Final connected graph: 4,314 records and 2,324
  eclasses including auxiliary tables.
- A second identical compiler invocation reuses all seven passes while checking
  their artifacts and health. The failed legacy artifact is rejected.
- The older raw-native whole-Pass-3 discrepancy is not resolved. Native small-case
  checks, cross-layout projection and same-artifact audits are separate claims.

## Reproduce

```sh
CARGO_TARGET_DIR=/tmp/eggfuse-ripen-build CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo build --offline --bin egg_layout --example eggfuse_frontend \
  --example lhs_ast --example structural_key_source

/opt/homebrew/bin/python3 -B -m eggfuse.compiler \
  --source out/known-static-compile/source.egg \
  --bake-source /Users/mineralsteins/.codex/worktrees/d569/egg_layout/out/shared-keys-sum-bits-cpu/bake-source.egg \
  --tools /tmp/eggfuse-ripen-build/debug/examples \
  --solver-python /Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python \
  --backend cell-cpu --multi-stage-cell --stop-after device \
  --output out/local-sat-connected-compile

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --compiled out/local-sat-connected-compile \
  --output out/local-sat-connected-bench-repeat --audit
```

Use a fresh benchmark output directory. `--marker-locality all` reproduces the
previous automatic layout. Do not time alongside compilation, tests or audit.
Compact evidence: `bake_locality_repair_results.json`. Full local outputs:
`out/local-sat-{healthy,connected}-{compile,bench}`. Failed interim compilations
are preserved in their pass directories, not counted as successful caches.
