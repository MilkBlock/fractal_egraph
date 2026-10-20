# Automatic negative lookahead in the actual GPU matcher

The requested 5% search reduction was exceeded on the **complete raytrace
Pass 3 snapshot and schedule**, with all 547 original rules admitted. This
is the persistent cell-assisted-egglog runtime, not a synthetic Cell example
or pristine upstream egglog.

Each configuration ran once, sequentially, on the same Apple M3 Max.
The ablation changes only `CELL_REFUTATION`; it preserves the Join order,
Cell sat-skip, native Run boundaries, guard semantics, and effect commits.

| Uninstrumented measurement | Off | On | Reduction |
|---|---:|---:|---:|
| Whole inference | 16.109678 s | 14.447312 s | 10.32% |
| GPU execution, including index maintenance | 5.804168 s | 4.025359 s | 30.65% |
| Setup, excluded from inference | 26.227037 s | 27.849243 s | slower |

Independent phase-timing diagnostics separate maintenance from search:

| GPU phase | Off | On | Reduction |
|---|---:|---:|---:|
| Matching, including routing and refutation | 5.689447 s | 4.004073 s | **29.62%** |
| Table/index maintenance | 0.061282 s | 0.052407 s | 14.48% |

Phase timing splits command buffers; its end-to-end time is **not** used
for the uninstrumented speedup. No atomic search counters were enabled in
either timing pair. Setup and serialization are excluded in both lanes.
These are observations on this workload, not a universal 5% guarantee.

Some ruleset search times from that diagnostic (all scheduled executions,
not a selected fastest iteration):

| Ruleset | Original Run count | Off | On |
|---|---:|---:|---:|
| type-analysis | 476 | 0.949549 s | 0.268062 s |
| state-edge-passthrough | 6 | 0.650502 s | 0.155562 s |
| always-run | 1798 | 2.163791 s | 1.879465 s |
| all-optimizations | 2 | 0.303827 s | 0.135115 s |

## What changes

Suppose a partial binding already knows `x`:

```text
P(x) → enumerate Q(x,y) → much more work → require R(x,z)
```

If an existing index on `R` proves that no row can have this `x`, the whole
remaining branch is impossible. Check that before enumerating the intervening
Join. The same idea applies when more coordinates become known deeper in the
search.

`src/refutation.rs` automatically compiles these necessary conditions:

1. Derive which slots are bound after each atom and its primitive computations.
2. For each later atom, select an existing tuple index whose key coordinates
   are all known. Do not add new indexes.
3. Skip the immediate next atom, which the ordinary Join already checks.
4. Check a given atom/index projection once along a path. A stronger projection
   can become available at a later stage.
5. Read its hash bucket. **Empty rejects the branch; nonempty proves nothing**
   and continues the original complete matcher.

Indexes are already shared across queries by `(table, key columns)`.
The added representation is static per query/anchor/stage, not an
instance/history cache. It adds no fact rows or per-instance negative markers.
The compiler generated 6,181 lookahead checks for this workload.

## Why there are no new false negatives

Every complete binding must include a row for each LHS atom. Its projection
onto already-bound coordinates must therefore appear in that atom's index.
If its bucket is empty, no such row exists.

Hash collisions and retired rows can make an impossible key's bucket nonempty.
That only loses an opportunity to reject. The refuter never treats nonempty
as a proof that a match exists, never joins incompatible partial bindings, and
never treats an unknown Cell as impossible.

Native committed inserts, updates, union/rebuild and visibility changes feed
the existing complete GPU mirror. The refuter runs after the same patch/index
maintenance already required by the original matcher. It stores no negative
answer across runs. Integer keys use the same exact two-word representation
as the normal index, including computed values that have no native intern ID
yet. Seminaive ownership and timestamps are checked by the unchanged matcher.

The fast path is the compact binding executor. General certified Cell-segment
execution retains its existing matcher. This result uses **query necessary
conditions**, not an additional proof derived from RipSatCell closure.

## Verification and counters

- Both uninstrumented lanes and both phase-timing lanes pass full comparison
  against the saved same-commit CPU and GPU-without-joint controls: **699,068
  rows**, **524,631 mapped e-classes**, including values and visibility.
- Every original Run retains the same native effect protocol. Both timing
  lanes have 2,761 GPU dispatches, 2,425,040 row notifications, 239,476 union
  notifications, and identical downloaded binding bytes.
- Full native-frontier audit passes **478,136 queries**.
- In the separate audit with atomic counters, the refuter performs
  **27,328,851 necessary-key checks** and rejects **9,279,340 branches**.
  The remaining search enters 44,859,847 indexed Join stages, visits
  142,640,189 index links and 140,473,864 live candidate rows.
  These are repeated operations, not unique rows or committed mutations.
  Without an off-counter run, they are not a percentage reduction in probes.
- Focused native-oracle regressions exercise insertion, union/rebuild,
  deletion/reinsertion, computed signed 64-bit keys, backtracking, combined
  rulesets, subsume, globals and independent incremental watermarks.
  Final layout-10 validation passes 8 library tests, 9 full-GPU tests, and
  22 benchmark workflow tests.
- Production Tiger extraction passes the same-commit CPU/GPU selected-DAG
  check; their extracted `.egg` output hashes are also identical. Unchanged
  native/CPU extraction controls are reused; the GPU output is freshly extracted.

The pre-existing mismatch against **fused native** execution is preserved;
this experiment does not claim to fix it. Same-commit equality and the fused
comparison are reported separately. The three-lane Tiger gate remains false
and the extraction command retains its failure exit status; only the
independent same-commit CPU/GPU gate is true. Both results are recorded in
`results.json`, not silently reclassified as a full upstream pass.

## Cost and limitations

For `Q` queries, at most `P` atoms per query, and `L` selected checks, the new
static representation is `O(Q P² + L)` words. Each check hashes its known
coordinates and reads one index head, `O(key width)`, with no linked-row scan.
It reuses the existing binding workspace and index storage. Compiling checks
currently scans the existing index specifications; this is setup work.

If all necessary keys exist separately but cannot participate in a common
binding, this filter may do nothing. It does not yet compute the intersection
of two possible-value domains, exploit stronger Cell-specific impossibility
proofs, or cache failed partial Joins. The worst-case original search remains.

Final worker protocol is layout 10, with 46 counter words: older workers fail
installation rather than accepting an undersized diagnostics buffer. The
saved timings/audit precede this protocol-number-only guard; the search
algorithm is unchanged.

## Reproduce

Use `skills/cell-assisted-egglog-bench/scripts/bench.py`; there is no parallel
benchmark driver. The preserved CPU/control directory is:

```text
/Users/mineralsteins/.codex/worktrees/cell-assisted-egglog/egg_layout/out/pass3-compact-layout
```

Run the standard `run --all-rules --joint-sat --reuse-controls DIRECTORY`
once with `CELL_REFUTATION=0` and once with its default enabled, using different
output directories. Keep `CELL_QUERY_TIMINGS` and `CELL_SEARCH_COUNTERS` unset
for inference timing. Use the same pair with `CELL_QUERY_TIMINGS=1` to isolate
matching. Run `audit --all-rules --joint-sat` for native incremental query
verification; `CELL_SEARCH_COUNTERS=1` adds work counts, not valid timing.

Raw reports are in `out/refutation-{off,first,search-off,search-on,audit}`.
The compact tracked `results.json` retains input/binary/worker fingerprints,
commands, complete graph checks, rule-set timing and counter evidence.
