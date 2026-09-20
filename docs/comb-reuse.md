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
