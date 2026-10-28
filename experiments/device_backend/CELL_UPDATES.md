# Parameterized multi-candidate Cell updates

The subsequent [v3 boundary recipe path](CELL_SPINES.md) also covers observed
`terms` seed programs. The measurements on this page remain the v2 ablation.

This extends the analyzer-exported v1 fresh-Cell recipes with a v2 executable
update program for the real Pass-3 extraction rules. Existing results, multiple
spines per owner, multiple candidates and multiple TCPair witnesses are allowed.
The runtime loads the operations from the artifact. It does not run the generic
helper LHS binder or the original helper RHS bytecode interpreter on this path.

## Evidence and typed contract

Native Analyze now exports the `set` witness's previous/proposed/actual rows,
and an example/count of requests with a known previous value. This separates
an observed function update from merely observing a rule name. The exporter
requires this evidence for v2 and validates all helper and comparison rules.

`update_recipe.py` exports three helper programs from the typed native AST:
initialization, conditional update and negative-cost failure. Parameters are
exact row fields identified by role/column. The artifact also retains the
complete comparison contract, dependency generations and native request-order
requirement. Hash validation alone is insufficient: load re-derives the typed
contract and compares the exported instructions. Changes to the rules, merge,
guards or instructions reject the old artifact.

A root owns **bags of correlated witnesses**, not a single winning cost:

```
PotentialExtractedExpr(root, candidate class)
ExtractedExpr(root, current result class)
TCPair(term, cost, class) rows for both classes
```

Spines producing the same root feed the same existing projection. The canonical
class membership and its row generations are refreshed after comparison/union.
No new Cell is baked per numeric value, candidate count or permutation of
spines. The original eight Pass-3 seed rules remain unchanged.

## Device execution

At each original helper stage:

1. Use the resident owner/function map and complete TCPair projection. Select
   only groups whose row generations satisfy the original semi-naive frontier.
2. A group's candidate minimum and old maximum can prove that **no** strict
   improvement exists. Skip such a group without enumerating its product.
   Negative-cost checks similarly use the complete candidate minimum.
3. For the remaining groups, retain every correlated witness combination,
   test the exact signed i64 guard, and prepare a compact action payload.
4. Sort enabled requests by the original rule/delta/seed/witness order.
5. Execute the exported constructor/set/failure instructions. The existing
   block merger preserves each ordered `set`, materializes the required
   `Smaller` expressions, runs comparison closure and publishes at the original
   stage boundary.

This is an exact continuation from known ports. It still performs numeric
checks and reads witness arrays; it is not zero-work matching. Candidate
multiplicity is not reduced to a single winner. Remaining products can still
be large; existing backend capacity checks/fallback remain, with no added
sampling budget or heuristic cutoff. Unknown/unsupported rule environments
cannot install this certificate. Ordinary finite Run uses the original path.

The extrema optimization belongs to the shared exact query projection, not to
a claim that one historically observed Cell proves all future impossible
matches. No neural prediction is used here. Helper query entry has a defensive
trap when v2 is active; its visit counter must remain zero.

## Why keep repeated set requests?

With old A(10), then B(7), then C(5), the merge constructs
`p=Smaller(A,B)` and `q=Smaller(p,C)`. Comparison can prove p=B and q=C;
those constructor facts still exist. Dropping straight to C can preserve the
winner while losing observable facts. The actual helper initialization may
also introduce sentinel comparisons. V2 retains those too.

The requests and recipes can remain local until stage publication. Preserving
this evidence does not require a global rebuild after each request.

## Reproduction

Existing v1 files remain supported. Generate a v2 file with `--updates` for
both fresh recipes and updates, or `--updates-only` to isolate the update
program. The latter is the measured full-Pass-3 configuration:

```sh
python3 -m tools.cell_spine.executable \
  --analysis ANALYSIS/analysis.json --core NATIVE_RUN/core.json \
  --source TRAINING.egg --output updates.json --updates-only
python3 experiments/device_backend/run.py \
  --source PASS3.egg --build BUILD --output NEW \
  --cell-program updates.json --stage-contexts --matrix-saturate \
  --check-every 0 --save-state --capacity 1048576 --match-capacity 1048576
```

`test_cell_updates.py --build BUILD --analyzer target/debug/egg_layout
--source PASS3.egg --output NEW` copies the real Pass-3 declarations/rules,
uses small training facts, calls Analyze, exports a file, and starts new workers
on eight held-out input/schedule cases. It additionally checks fresh/update
coexistence, overflow/negative rollback and forged instructions. `--evidence-from`
can reuse the unchanged native training evidence. No copied final graph is used
as the executable template.

`extract_pair.py` reuses the exact snapshot importer and production Tiger/strict
selected-DAG checker for two GPU endpoints. Both lanes are labelled GPU; import
runs no inference/rebuild. This avoids mislabelling the control as native CPU.

## Results: 2026-10-28

Actual GPU-owned backend, one run per configuration, the same worker/frontend
binaries, original complete Raytrace Pass-3 schedule, no ridge, stage contexts
and matrix saturation enabled. Building, final serialization, graph checking
and extraction are outside inference timing.

| Measure | Update program off | Update program on |
|---|---:|---:|
| Full inference | 41.1157 s | 39.9954 s |
| GPU preparation | 4.1635 s | 4.0618 s |
| GPU match | 9.2100 s | 9.2384 s |
| GPU commit/rebuild | 16.6422 s | 15.9681 s |
| Joint terms program GPU time | 5.5705 s | 5.0454 s |
| Generic helper witness checks | 2,179,332 | 0 |
| Exact update guard checks | — | 113,916 |
| Witness-pair checks eliminated by complete extrema | — | 2,065,416 |
| Prepared helper actions | — | 113,916 |
| Actions whose input includes an existing function result | — | 42,713 |

**Full inference improves 2.72% (1.028x); this is not a large end-to-end gain.**
All 2,179,332 prior helper checks are accounted for by eliminated pairs plus
remaining guard checks. This is not the whole program's candidate denominator.
The overall GPU match phase is essentially unchanged: other rulesets dominate,
while this change also reduces helper payload and RHS interpretation work.

- Full off/on graphs: **699,784 rows / 524,742 eclasses**, complete facts,
  values, visibility and anchored equivalence equal. Original schedule and
  internal Run history equal.
- Production Tiger: **29 functions / 8,019 selected rows**, exact selected DAG
  equal, without normalization that hides differences.
- Eight real-rule adversarial fixtures pass full native graph equality. Multi-
  witness, late union, sequential improvements and finite Run are included.
- The old v1 suite passes all 18 checks. Mixed programs and failure rollback pass.
- The original full-Pass-3 native-vs-device mismatch remains. Off/on equality and
  Tiger equality do **not** claim that mismatch has been fixed.

Reports: `out/cell-update-pass3/{full-control,full-raytrace,same-build-compare.json,
extraction}` and `out/cell-update-real-tests/results.json`; compact committed
results are in `results/cell-updates.json`.

A separate initial-graph run with all fresh recipes enabled matched native
(27,793 rows / 21,655 classes), but took 6.13 s versus a stored 3.55 s stage
control. That historical, different-build comparison diagnoses a regression;
it is not the paired v2 update benchmark above. It exercised no existing-owner
updates. Fresh-recipe admission/validation and comparison seeding still add
work at every private iteration. The update-only file is an explicit ablation,
not an automatic workload-dependent heuristic or a concealed library shrink.

## Remaining work

- Seed terms still use the existing marker/port matcher; arbitrary boundary
  grafts and arbitrary rulesets do not have this update certificate.
- Overlapping cost ranges still require enumerating surviving witness products.
- Ordered helper publication is still serial on the GPU. Parallelization must
  preserve per-key custom-merge ordering and shared constructor identity.
- There are still hundreds of private stages in deep input DAGs. This work
  installs a correct update program; it does not yet collapse those stages into
  one large matrix execution or establish high GPU occupancy.
