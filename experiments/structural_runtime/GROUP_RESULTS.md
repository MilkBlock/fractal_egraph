# Event-bound condition handles and exact candidate groups

This continues the real saved raytrace Pass-3 runtime experiment, using the
same 75 supported always-run queries. It does not add a RipSat saturation proof
or modify the egglog kernel. The queries already satisfy the checked contract:
a complete anchor binds every variable shared across LHS atoms; each remaining
atom has one bound key, with full typed/alias/literal constraints retained.
Unsupported queries keep the existing full GPU matcher and original effects.

## Which knobs changed

**Execution granularity.** Each Run interns actual `(projection, handle)`
conditions shared across its affected candidates. Its current existence is
checked once. Candidate conjunctions share canonical condition prefixes;
permutations and repeated conditions normalize, but candidate bindings are never
merged. An exact negative rejects every dependent candidate. Complete correlated
witness rows are read only after all conditions pass. Original RHS, timestamps,
Run boundaries, full match coverage, and complementary native checks remain.

The existing projection index is complete for this admitted class. Therefore
predicting these existence bits is unnecessary: its exact current state is
cheaper and stronger. This is **not** an assertion that a small Bake library
covers arbitrary graph queries. No negative conclusion is made from missing Cell
coverage, and no approximation is used to discard candidates.

**Context representation.** Native commit events bind each matching anchor's
slots to stable condition handles. Later candidates follow these handles rather
than repeatedly mapping their values to projection buckets. Each bucket keeps
its current rows, version and immutable cached answer together. Typed decoding
is compiled once into column positions, constant checks and alias comparisons;
neither commit routing nor each candidate rebuilds a BTreeMap of variables.
Shared-prefix truth is Run-local; native bindings and original slot maps remain
separate.

**Local updates.** Static table subscriptions route a commit directly to its
projection indices and anchor consumers. Function replacement, union/rebuild,
removal and subsume update affected buckets and anchor connections. `touch`
invalidates only that bucket's cached payload; unrelated handles keep their
answers. Timestamp-only changes still refresh payloads for semi-naive execution.
Handles are not recycled, so old IDs cannot accidentally name a different
condition. Empty historical handles currently remain allocated.

**Prediction and matrices.** `--condition-execution groups` bypasses speculative
score generation and its unused feature maintenance for these queries; other
rules still use the ordinary GPU matcher. It does not train a new learned
"expected saved work" model or pretend one is needed for already exact bits.
The shared-matrix path remains available under `roots`, for testing cases where
scores can actually change work. This experiment is not a claim that most
reasoning has moved to Tensor Cores.

## Complete Pass-3 result

Each configuration ran once, with the same binary, model, original schedule,
ordinary GPU worker and original sorted submission protocol. The grouped lane
also passed independent native-query audit. The common native/GPU-off controls
were explicitly reused from history. No best-of timing was selected.

| Metric | Per-candidate score ordering (`roots`) | Exact groups (`groups`) |
|---|---:|---:|
| Complete inference | 21.841 s | 20.209 s |
| Provider maintenance | 1.593 s | 1.260 s |
| Provider query handling | 1.397 s | 1.068 s |
| Provider total | 2.990 s | 2.328 s |
| New GPU prediction scores | 827 | 0 |
| Materialized projected answers, including empty answers | 1,063,742 | 855,978 |
| Materialized witness rows | 958,749 | 958,726 |
| Complete returned bindings | 1,502,588 | 1,502,588 |

Single-run complete time decreased **7.47%**; measured local maintenance plus
query time decreased **22.13%**. Other native/GPU phases also varied, so the full
wall-time difference is not attributed solely to one inner operation or a new
Cell theorem. The unchanged historical ordinary-GPU control is still faster
(17.714 s); the provider therefore remains optional.

Groups process **1,317,438 shared conditions** and **2,274,404 candidate-to-condition
references** across Runs. There are **151,631 newly evaluated conjunction steps**
and **1,936,569 reused/single-condition steps**. They reject **544,707 candidate
visits before payload access**. These are Run-scoped counters, not that many
unique Cells. Importantly, actual witness-row copying decreases by only **23
rows**: the existing executor already shares many positive answers. Do not call
the 544,707 rejected visits saved row copies or new Cell proof credit.

After grouping, `truth_index_reads` in the raw compatibility report means direct
reads of stable condition slots (1,273,905), **not new hash-key searches**. Payload
references and conjunction metadata are separately charged. Per-rule depth
histograms/fixed-weight reach curves are in `group-results.json`; grouped depth
zero means rejection by the preceding group phase, whose work is reported
separately. These curves are not native Free Join depths and do not make group
filtering free.

Original native row events, union events, ruleset run count, ordinary GPU
dispatch count and full returned bindings agree. Ordinary-worker upload differs
by only 23,440 bytes (184,175,964 vs 184,152,524); the subscribed auxiliary feature
feed differs between these paths; downloads agree. The comparison records this difference
rather than asserting identical traffic.

Current storage contains **503,794 stable condition handles**, of which
**130,342** are nonempty, plus **604,543 live anchor-slot connections**. These
are logical counts, not RSS or a memory-compression claim. Recycling unused
empty handles is future work; no row-times-plan capacity is preallocated.

Validation: **26 Rust tests**, including direct decoding against the original
binding decoder, conjunction permutations/aliases, late facts, multiple
witnesses, function updates, union, subsume, and unrelated-cache survival;
**134,850 native full incremental binding checks**; exact final **699,068
original table rows** with function values/equivalence/visibility; production
Tiger selected-DAG equality for **29 functions**. No original table is ignored.
The existing common-submit/pristine distinction remains unchanged.

## Reuse the benchmark workflow

Use the existing `run.py` invocation and same exported matrix model, with:

```sh
--condition-execution groups --coordinates markers --matrix-layout shared
```

The ordinary GPU worker is still required for unclaimed queries. `audit` checks
the complete provided bindings; `gpu` is the uninstrumented timing lane. Use
`roots` for the same-binary control. `--compare-execution ROOTS_RUN` reads those
results and writes all per-rule distributions, shared-group work and phase times
without retiming. `--extract` calls the existing bench skill's production Tiger
checker. Fingerprint-matched completed lanes and historical controls are reused.

The final pair is `out/condition-handles-{roots,groups}`. Earlier prototype runs
`out/condition-grouped` and `out/condition-groups-slots` preceded the stable-handle
representation and are retained; they are not mixed into this paired result.
