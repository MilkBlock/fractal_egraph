# Shared bucket summaries: a second negative filter

This extends the empty-bucket lookahead in `aa7e3e9`. The selected default
adds **key fingerprints**, not Cell-specific impossibility certificates.
The stronger **shared-value domain intersection** is implemented but remains
opt-in because its first measurement did not establish a net benefit.

## Actual runtime result

Full saved raytrace Pass 3, all 547 original rules, unchanged schedule and
native effect commits, one sequential run per configuration. Both lanes
already have the previous empty-bucket filter and joint sat-skip. Only
`CELL_REFUTATION_SIGNATURES` changes in each pair.

| Uninstrumented measurement | Fingerprints off | On |
|---|---:|---:|
| Whole inference | 15.579166 s | 13.444418 s |
| GPU execution including maintenance | 4.222318 s | 3.096988 s |
| Setup, excluded from inference | 28.690048 s | 31.307779 s |

Whole inference decreases **13.70%**, GPU execution **26.65%** in this pair.
The previously reported 14.447312 s is an older measurement of the first
filter; it is not substituted for this pair's control.

A separate phase-timing pair (no atomic counters) measures the mechanism:

| GPU phase | Off | On |
|---|---:|---:|
| Matching including refutation | 4.122091 s | 3.031531 s |
| Table/index/summary maintenance | 0.087606 s | 0.089976 s |

Search decreases **26.46%**; extra maintenance is **2.37 ms** over the whole
schedule. Phase timing changes command-buffer boundaries, so its total
inference time is not used as the ordinary speedup. All four full graphs pass
the saved same-commit CPU and joint-off reference checks.

The new default passes all **478,136 native-frontier query checks**. Comparing
its separate counted audit with the saved first-filter audit (same input and
schedule; these are work counts, not timing):

| Repeated operation | Previous empty-bucket filter | With fingerprints |
|---|---:|---:|
| Entered indexed Join stages | 44,859,847 | 26,650,658 |
| Live candidate row visits | 140,473,864 | 48,033,034 |
| Necessary-key checks | 27,328,851 | 12,200,432 |
| Refuted branches | 9,279,340 | 7,803,841 |

Of the new refutations, 1,465,413 are specifically fingerprint failures after
a nonempty bucket. This number is a subset of the total, not additive. Earlier
parent rejection prevents later branches from being generated, so total
refutations can fall while search work falls much further. Live row visits
decrease 65.81%; they are not distinct facts or committed mutations.

Final library tests pass 9/9 and full-GPU tests pass 10/10 with both filters
enabled, including cross-query index sharing, alias-enabling union, new
bridges, deletes/reinserts and index rehash. Benchmark workflow tests pass
22/22. The new domain regression deliberately supplies its needed projection
through another query: without that pre-existing index the filter stays
inactive instead of inventing an index or rejecting a match.

The fingerprint variant has 174 shared summaries and **12,034,984 bytes
(11.48 MiB)** of live GPU summary storage at the final boundary. This is
additional storage, not compression and not process RSS. It does not add
per-row linked indexes or per-query/per-instance histories. Index count stays
399. Cold compilation/setup remains material and is shown separately.

## Key fingerprints

An ordinary bucket can be nonempty because an unrelated key hashed to the
same bucket. The first filter cannot reject that case. For each bucket in
the selected existing indexes, retain one 32-bit word:

```text
on indexing a row:
    h = the normal typed tuple-key hash
    bucket_summary[bucket(h)] |= 1 << (mix(h) & 31)

on checking a required key:
    h = the same typed tuple-key hash
    if bucket is empty: reject
    if its summary does not contain bit (mix(h) & 31): reject
    otherwise: continue the complete Join
```

A real key must set its bit. Bucket collisions and bit collisions can only
let extra impossible candidates survive. This is not a test that the key
exists: passing the filter never bypasses normal binding/guard validation.
All rules referencing the same tuple index share the same summary.

## Optional domain intersection

For `Frame(x,z), L(x,y), R(y,z)`, an anchor can bind `x,z` while `y` is still
unknown. Individually both necessary keys can exist, yet `L` may only offer
`y=10` and `R` only `y=20`.

The compiler finds these shared unbound slots from typed LHS wiring. Using
existing indexes, it ORs hashed value bits for each `(index, output column)`.
At the anchor, it intersects the two conditioned bucket summaries. Zero
intersection rejects every continuation; nonzero remains inconclusive.
The value hash uses exact integer words for i64 and canonical native identity
for equality sorts. Typed slot identity prevents comparing unrelated columns.
Identical projections are shared across rules; no Cell absence is evidence.

The domain-only exploratory run compiled 747 checks and 128 summaries:
14.997733 s inference, 4.053599 s GPU execution, 9,347,932 summary bytes.
GPU time is close to the older empty-bucket result (4.025359 s); this was not
a same-binary domain-only ablation, so no validated domain speedup is claimed.
The option remains off. It is limited to anchor-available conditioning keys;
it does not search for arbitrary relational inconsistency or add indexes.

## Incremental correctness

Both modes use the same GPU summary buffer and index-update pass. Every new
canonical row ORs its contributions before the next query. Deletes leave
extra bits until the existing index rebase rebuilds summaries from live rows;
clearing individual bits would be unsafe when several rows share a bit.
Union/rebuild rekeying therefore adds all newly possible keys/values without
relying on a historical negative cache. Hash collisions, stale positive bits
and ignored constraints enlarge possibilities, never remove them.

Allocation follows the existing per-table bucket counts, only for projections
actually referenced by the compiled filters. Domain intersection costs two
summary reads and an AND per check; a key fingerprint adds one summary read
after the old empty-bucket test. Maintenance costs one atomic OR per applicable
summary per indexed row, plus zeroing on normal index rebase. No new passes
over historical matches, representative sampling or candidate budget is used.

Final worker layout is 12, reserving 64 diagnostic counter words. Existing
Metal argument slots 0–22 stay fixed; summaries use slot 23. An early prototype
accidentally shifted the output/counter slots, causing the full-graph gate to
fail. That run (`out/refutation-domain-first`) is retained as a failure and
excluded from all performance claims. The repaired domain run passes.

The current same-commit versus fused-native discrepancy is unchanged. Do not
interpret this optimization as fixing that separate protocol difference.
Audit, focused regression and Tiger results are saved in `summaries-results.json`.
Fresh GPU Tiger extraction passes the same-commit CPU/GPU selected-DAG check
and produces the same output hash. The fused-native three-lane gate still
fails, and its failure exit code is preserved.

## Reproduce with the existing bench skill

- Default: empty-bucket filtering plus key fingerprints.
- `CELL_REFUTATION_SIGNATURES=0`: previous empty-bucket filter only.
- `CELL_REFUTATION_DOMAINS=1`: also enable shared-value domain intersections.
- `CELL_REFUTATION=0`: disable all three filters and summary maintenance.
- General/noncompact executors do not allocate these unused summaries.

Use the normal `bench.py run --all-rules --joint-sat --reuse-controls ...`
with a fresh output directory for each configuration. Set
`CELL_QUERY_TIMINGS=1` only for phase diagnostics. Use
`CELL_SEARCH_COUNTERS=1` with `audit --all-rules --joint-sat` for counted
native-query verification, never for ordinary timing. Full source commands,
binary/worker/input fingerprints and saved control identities are in the JSON.
