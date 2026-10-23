# Integration checks, 2026-10-23

Actual native runtime / shared CPU-GPU event loop. No new performance credit is
claimed from the certificate registry itself. Full numbers: `results.json`.

| Case | Result |
|---|---|
| Automatically analyzed math assoc/commute, new input bindings, fresh file load | Native smooth matches **0**, fallback **0**, one whole saturation skipped; full constructor/equality graph agrees with native |
| Frozen Analyze If/Single catalog, new binding | One proved macro published, one native fallback retained; checks pass. This is not whole-ruleset coverage |
| Automatically analyzed `extent :merge min`, new binding and a later set | Both checked saturation scopes run; 3 numeric updates; final checks and native-graph regression pass |
| Fully renamed selection operators/functions/rulesets | Same checker and execution; no application-name dispatch |
| Corrupt/missing wiring, guards, coverage, unsupported extra helper | Loader refuses authority |
| Missing coarse forest row/equality, late union and cross-Cell matching | No premature marker; native fallback and complete bindings retained |
| Loaded parameterized library in shared CPU and GPU event loops | Matches native at every boundary; 7 stored programs, 0 query-plan rediscoveries |

The ordinary `analyze --bake-certificates` path caught and fixed a preexisting
history-export error: `Port::Parent(p, output)` uses `p` relative to the consumer's
parent list. Indexing `records[p]` directly produced wrong provenance or an
out-of-bounds panic. The multi-parent math end-to-end regression now covers it.

## Raytrace Pass 3

Used the existing bench skill, one new GPU run and unchanged historical controls;
no new pristine or CPU timings. Worker: `out/commit-marker-worker/matrix`.

- Same-submit CPU comparison: **699,068 original rows / 524,631 mapped classes**, exact
  facts, function values, equalities and visibility.
- GPU without joint proofs: same final graph.
- Production Tiger CPU/GPU selected DAG: equal, 29 functions.
- Registry contains 14 complete-query certificates, 17 candidate-query certificates,
  one selection closure and one min-plus closure. Queries are not closure credit.
- 12 original joint saturation scopes, all handled through parameterized dataflow;
  **0 fresh whole-selection macro hits**. Comparison closure handled 1,185 inner
  saturations. Do not describe those as 12 fresh whole-Cell macro hits.
- This run: 14.7541 s inference, 2.7547 s setup. No same-version pre-change timing
  was collected, so this is **not** an additional speedup or regression claim.
- The existing fused-native mismatch remains in Smaller/TCPair/TermConcat. The
  overall report therefore retains `complete: false`. It is not hidden by the
  successful same-submit CPU/GPU and extraction checks, nor called pristine parity.

Reproduction uses the versioned skill:

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py run \
  --all-rules --joint-sat --worker out/commit-marker-worker/matrix \
  --reuse-controls /Users/mineralsteins/.codex/worktrees/cell-assisted-egglog/egg_layout/out/pass3-compact-layout \
  --output out/NEW --no-cell-metrics
```

120 relevant Rust suite tests (including shared graph-comparator regressions),
59 bench-script tests and 12 marker-compiler tests passed. CPU/GPU loaded-library
and competing-update cases were run with `CELL_MATRIX_WORKER` set.
