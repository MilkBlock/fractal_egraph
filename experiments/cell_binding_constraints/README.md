# Version-valid Cell interfaces as negative GPU evidence

This experiment runs the **actual full raytrace Pass 3 GPU runtime**, with all
547 original rules admitted, native RHS/merge/rebuild, and the existing CPU Cell
service unchanged. It is not a standalone matcher or a replacement for all
Analyze Cell semantics. No original rule or egglog kernel was modified.

The feature is **opt-in**. The tested numeric envelopes remove extra work, but
have not demonstrated a net speedup. The ordinary backend remains the default.

## Contract and implementation

The installed parameterized Cell transfer programs export typed port shapes.
They do not export sampled positive histories as if they were complete domains.
Two implementations consume this wiring:

* `CELL_BINDING_CONNECTIONS=1`: a complete native function key gives either no
  visible row or one full binding. The existing sparse GPU Cell registry caches
  the connected pair, including its slot equalities. Key watches include the
  first miss. Native changes wake the affected instance; anchor generations and
  dependency revisions protect cached state. Uncovered/invalid interfaces fall
  back to ordinary Join. This version only exports functional two-atom
  connections, not arbitrary multi-candidate relations.
* `CELL_BINDING_RANGES=1`: numeric ports supply conditioned lower/upper envelopes
  over **all current rows** in a native index fiber. Cell program provenance
  selects the ports; native index maintenance supplies completeness. This is
  broader than a single Cell's internal closure, not a proof that the whole
  candidate lies in a particular saturated Cell. Multiple consumers reuse each
  `(index projection, column)` envelope.

Both perform only early rejection. The original Join still constructs complete
bindings and applies guards exactly. Ordinary Run watermarks, anchor ownership,
RHS ordering, and effects are unchanged. Two independent marginal intervals
never establish that a correlated tuple exists.

For example, if every current `TCPair(_, cost)` with output `p` has `cost >= 20`,
then a consumer requiring that output and `cost < 10` has no binding. One known
cost of 20 would **not** justify this conclusion; another alias may have cost 3.

Numeric envelopes share the existing GPU index-summary buffer. The complemented
minimum and maximum both use 32-bit atomic max. i64 values outside the exact
interior of the i32 coordinate are mapped conservatively to negative/positive
infinity. Equality at either clipped endpoint is inconclusive, including strict
comparisons. The final Join still uses exact i64 arithmetic. No data-dependent
sampling, selected representatives, extra table indexes, or time budgets enter
this proof.

Each native update packet is fully processed before its queries. Inserted and
canonicalized rows widen envelopes before use. Deleted/retired rows may remain
in an envelope until index rebase, which only causes false positives; rebase
clears and reconstructs envelopes from all current visible rows. Thus a range
need not be the *tightest* current interval to remain a valid upper bound on
possible bindings. No additional CPU binding-list transfer is required.

## Measurement

See `results.json` and `depth.csv`. All off/on comparisons use the same Rust
binary, worker, snapshot, schedule, original refutation mode, and Cell query
routing. Native/CPU/GPU-off graph controls were reused from the validated
`out/pass3-compact-layout` run in the sibling `cell-assisted-egglog` worktree.
Each configuration ran once; diagnostic counters/timers are not speedup data.

| Metric | Range off | Range on |
|---|---:|---:|
| Eligible multi-atom seed evaluations | 13,278,476 | 13,278,476 |
| Productive seed evaluations | 492,823 | 492,823 |
| Live candidate row visits | 48,033,034 | 48,015,839 |
| New range checks | 0 | 36,948 |
| New range refutations | 0 | 27,114 |
| Uninstrumented inference | 14.152401 s | 14.562461 s |
| Uninstrumented GPU device time, all phases | 3.078957 s | 3.132320 s |
| Worker setup, separately measured | 29.538398 s | 31.713732 s |
| Final allocated index-summary bytes | 12,034,984 | 15,180,712 |
| Native delta upload bytes | 102,414,848 | 102,414,848 |

Candidate visits fall **0.035798%**, while inference is **2.897% slower** in this
single-run comparison. Additional device summary storage is **3 MiB**, not RSS.
The singleton-connection trial instantiated 33 shared plans but produced **zero
new refutation checks** on the residual GPU workload. Its plans mostly served
queries already handled by the CPU Cell service; it is not enabled by default.

Separate phase-timing diagnostics (command buffers split, not production
speedup timings) measured index maintenance **0.077275 → 0.078856 s** and
matching **2.989437 → 3.070620 s**. The extra checks cost more than their saved
inner work in this workload; transport bytes are unchanged.

The range experiment reaches seven rules. Of its 27,114 refutations, 24,355 are
in `error-checking`; others are in `select_opt`, `switch_rewrite`, and `drop`.
A high success rate among range checks is therefore not evidence of broad Cell
coverage or large search savings. Most newly excluded work was shallow or cheap.
The expensive remaining searches need richer correlated port constraints.

Depth 0 is the valid delta anchor; depth d attempts the d-th non-anchor atom.
The reach probability comes from one maximum-reached-depth bin per seed
execution, not `entries / seeds` (branching can make the latter exceed one).

| Reach at least depth | Range off | Range on |
|---|---:|---:|
| 1 | 46.196725% | 46.082766% |
| 2 | 2.006051% | 1.996321% |
| 3 | 0.654593% | 0.641919% |
| 4 | 0.246685% | 0.246685% |
| 5 | 0.060429% | 0.060429% |

Per-rule seed counts and productive counts match. This controls rule mix, not
individual seed identities or output-retry identities. The CSV includes every
layer, fixed-rule-weight comparisons are in the JSON, and the skill checks
counter conservation before reporting them.

## Validation and reproduction

* All four fresh range off/on count/timing runs pass complete same-submit CPU
  graph checks: 699,068 rows and 524,631 entries in the equality-class mapping, including values and
  visibility. The prior fused-native discrepancy is preserved, not relabelled.
* All 478,136 incremental native-oracle queries pass with ranges enabled.
* Twelve GPU native-oracle regression tests pass with connections, ranges,
  original domains, and depth counters enabled together. New tests exercise
  missing-to-present keys, function updates, late aliases, subsume, index rebase,
  and i64 extremes. Thirty skill/cache/report tests pass.
* Tiger extraction was not rerun; no new extraction-quality claim is made.

Use `skills/cell-assisted-egglog-bench/scripts/bench.py`. For fresh candidates,
keep `--all-rules --joint-sat --reuse-controls <validated controls>` and change
only `CELL_BINDING_RANGES=0/1`. Set `CELL_SEARCH_COUNTERS=1` for the separate
counting experiment; unset it for timing. `CELL_QUERY_TIMINGS=1` is a third,
phase-timing diagnostic and does not establish an inference speedup.

Read the existing depth ablation without executing inference:

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py depth-pair \
  --before out/cell-range-depth-off --after out/cell-range-depth-on
```

The installed skill now treats Join-depth distribution as a required Cell/Join
assessment, keeps same-input historical controls, and distinguishes root-count
changes from savings inside an unchanged residual query workload.
