# Online Use / residual composition

`src/comb_reuse.rs` is invoked by `LayerStore::push` for each **validated** apply.
Native collection still drains at completed execution boundaries; this does not add
speculative matching or mutation skipping inside the egglog join engine.

## Construction

1. Actual producer occurrences index `owner`, yielding `UsePort(instance, member,
   output)` or a residual port. All ports of a completed Use, including interior
   outputs, remain addressable. No class-value scan across historical instances.
2. Form at most eight witnessed candidate regions: individual producer
   continuations, joint continuations, and three bounded dependency expansions.
   A known Use is expanded as a whole region for validation and retained as an
   atomic recipe reference. Layers do not gate the normal builder.
3. Canonicalize source schemas, ordered wiring, global aliases, exact required
   facts and produced row/equality effects. Outside proof origins stay in the
   instance context rather than the template key. Literal/guard source semantics
   and output roles remain in shared basic schemas.
4. Look up these candidates in the **already existing** dictionary. This is exact
   lookup of producer-anchored candidates, not unrestricted e-matching of every
   possible template or scanning every old instance.
5. Prefer the completed candidate with greatest immediate wiring-model saving.
   A definition is admitted after historical matching opportunities have accrued
   enough credit to cover its definition size. Credit is a heuristic, not realized
   savings; it cannot prove future reuse.
6. Publish the selected Use, update owner indexes, and retain its immutable parts
   and boundary references. Subsequent applies actually obtain UsePort references.
   Only afterwards are this event's new candidates learned. Existing definitions
   can acquire smaller recipes using earlier Uses as atoms.

The online composition dictionary and the older post-hoc interface/Fractal
catalog have separate IDs. Use(T) in the reuse view refers to the composition
recipe dictionary; it is not a claim that the old analysis catalog now performs
an arbitrary template search. Both keep the original source/effect evidence.

## Constraints and correctness

- Preferred Uses cover disjoint event sets. A larger Use may contain whole prior
  Uses. Partial overlap is not selected in this version.
- Regions must be dependency-convex. An omitted node on a path between selected
  members is not silently moved across a macro boundary. Convexity search has a
  4096-node budget; uncertain candidates are skipped and counted.
- Maximum region size 16 applies, three expansions, four direct producer seeds,
  eight candidate probes per apply, dictionary limit 4096 definitions. These are
  search budgets, not completeness or optimality guarantees.
- `verify` reconstructs internal and boundary port bindings, aliases, requirements,
  row/union effects, and exact member coverage from immutable native evidence.
  It also checks Uses only reference earlier definitions/instances.
- Coarse residuals retain explicit external bindings/effects. Reading a known
  older Use does not need a dummy forwarding node through the immediately previous
  layer. External requirements do not imply facts are destructively consumed.

## Measurements

The cost model counts composition instructions and port/effect references, not
bytes. Shared basic rule schemas are common to both encodings. Reports separate:

- raw wiring units;
- selected root wiring units;
- **used** dictionary units;
- all candidate-index units (including unused definitions);
- covered events, residuals, Use continuations and interior port reads.

Raw native occurrence payloads, verification indexes, inactive Uses referenced by
older records, and the Tier2 compatibility projection are still retained. Do not
interpret model savings as an end-to-end memory reduction or tier0 speedup.
A negative total including the candidate pool is reported without being clamped.

`EGG_LAYOUT_REUSE_ABLATION=1` adds a second end-to-end builder to the final report.
It uses the same certified history but permits candidates only in the backward
slice stopping at coarse introductions. Both discovery and selection obey that
restriction; this ablation does not hold the learned dictionary fixed.

```sh
EGG_LAYOUT_REUSE_ABLATION=1 cargo run --release -- analyze --recapture-tier0 \
  --source egglog/tests/math-microbenchmark.egg --rounds 6 --save-history \
  --output out/use-reuse-math
cargo test --test comb_reuse --test layer_rounds
```

Use the existing tools page and select `Use(T) / residual 复用`. It shows the
preferred roots and referenced sub-combinations, and reuses the same Typst/Graphviz
renderers. Every new snapshot adds `round-NNNN.reuse.dot`. Earlier snapshot kinds
remain available; old logs without reuse data report that it must be recaptured.

## Math six-round result

Both arms process the same 1529 native witnessed applies. These are model units:

| Admission | Covered events | Selected wiring | Used definitions | Candidate index |
|---|---:|---:|---:|---:|
| Interface only | 687 | 23886 | 5193 | 391056 |
| Stop at coarse frontier | 583 | 25890 | 1722 | 30297 |

The unrestricted builder performs 3519 later input-port reads through Uses,
including 3119 interior-port reads. It increases coverage but does **not** win on
selected wiring plus dictionary, and its speculative matching index is much
larger. This is evidence that a mandatory layer gate loses reuse opportunities,
not proof that removing it is already a good overall compression policy.
The 12-apply `reuse_coarse.egg` probe demonstrates repeated mixed-input A+B units:
interface-only covers 6 applies, while the coarse-frontier arm covers none.
See `comb-reuse-results.json` for all counters. Further work should target the
candidate pool and template admission, rather than hide that overhead.

## Online re-cut (2026-09-20)

Physical `(event, output)` and context event handles now remain stable when the
preferred cover changes. For compatibility the JSON variants are still named
`ResidualPort` and `ResidualContext`; these names no longer imply current ownership.
`locate` returns the current Use/interior member in O(1). Existing immutable Uses
remain valid witnesses even after removal from the preferred cover.

Every 64 validated applies, `repartition` computes an additive cut DP over existing
Use decomposition trees: retain a Use or expose its parts. It then tries removing
up to eight expensive dictionary entries, recalculating the cut each time. Shared
definition cost is charged once for each *actively selected* template, using its
self-contained flat pattern. A change is accepted only if selected wiring plus
active dictionary cost does not increase. This corrects the prior report which
charged every historically used template as though still selected. Historical
Uses, matching patterns, recipes and source evidence all remain resident and are
not included in that selected-codec metric; the candidate-index metric remains
separate. The dictionary-removal heuristic is not an exact global dictionary
optimizer. Discarded ancestor choices are not automatically revived; later apply
candidates may create new combinations from the current cover.

For n retained events, u retained Uses, p total decomposition-part references and
s total Use member references, one re-cut performs at most nine DP evaluations.
With balanced-tree sets its conservative time bound is
O(9(n + p + (u + g) log(u + g + 1)) + s), including scanning/sorting up to g active
definitions and rebuilding owners. Temporary storage is O(n + u + g); all retained
history and the dictionary are additional. This implementation scans the existing
hierarchy rather than updating a balanced tree's ancestors: periodic re-cuts can
therefore be quadratic across a growing stream. It does not implement rotations,
arbitrary overlapping DAG partitions, index eviction, or the earlier proposed
logarithmic incremental bound.

### Controlled Math6 history replay

Both arms replay exactly `out/use-reuse-final/math/history.json` (1,529 certified
applies, six rounds); tier0 is not re-executed in this comparison. The new native
analysis path runs in both arms. Set `EGG_LAYOUT_DISABLE_RECUT=1` for the control:

```sh
EGG_LAYOUT_DISABLE_RECUT=1 target/release/egg_layout analyze --replay-history out/use-reuse-final/math/history.json --output out/recut-math6/off
target/release/egg_layout analyze --replay-history out/use-reuse-final/math/history.json --output out/recut-math6/on
```

| Metric (model units, not bytes) | Control | Re-cut |
|---|---:|---:|
| Raw wiring | 31,248 | 31,248 |
| Selected wiring | 23,886 | 25,385 |
| Active dictionary | 5,015 | 2,669 |
| Selected wiring + dictionary | 28,901 | 28,054 |
| Candidate index | 391,056 | 392,807 |
| Covered events | 687 | 597 |
| Re-cut passes | 0 | 23 |

Selected codec improves 2.93%, but total with candidate index gets worse.
This is evidence for rejecting unamortized selected templates, not overall memory
compression or tier0 acceleration. Re-cuts also change later candidate discovery;
this is an end-to-end policy comparison on identical evidence, not a fixed-library
ablation. Single-run tier1 timings (0.197s vs 0.208s) are recorded only as diagnostics.
