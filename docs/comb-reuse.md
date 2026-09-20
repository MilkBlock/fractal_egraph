# Online Use / residual composition

`src/comb_reuse.rs` is invoked by `LayerStore::push` for each **validated** apply.
Native collection still drains at completed execution boundaries; this does not add
speculative matching or mutation skipping inside the egglog join engine.

## Construction

1. Store stable physical `(event, output)` handles. `owner` / `locate` resolve
   their current Use or residual for display and composition. Interior outputs
   remain addressable across re-cuts. No class-value scan across instances.
2. Form at most eight witnessed candidate regions: individual producer
   continuations, joint continuations, and three bounded dependency expansions.
   A known Use is expanded as a whole region for validation and retained as an
   atomic recipe reference where wholly included. Direct producer suffixes also
   permit local boundary rotation. Layers do not gate the normal builder.
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
   and stable boundary references. Subsequent applies resolve the current owner.
   Only afterwards are this event's new candidates learned. Existing definitions
   can acquire smaller recipes using earlier Uses as atoms.

The online composition dictionary and the older post-hoc interface/Fractal
catalog have separate IDs. Use(T) in the reuse view refers to the composition
recipe dictionary; it is not a claim that the old analysis catalog now performs
an arbitrary template search. Both keep the original source/effect evidence.

## Constraints and correctness

- Preferred Uses cover disjoint event sets. A larger Use may contain whole prior
  Uses. For partial overlap, remove the old Use and preserve its uncovered members
  as residuals; their cost participates in selection. No event is dropped.
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
- all candidate-index units (including unused definitions and probation patterns);
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
current preferred roots, with historical parts retained in details, and reuses Typst/Graphviz
renderers. Every new snapshot adds `round-NNNN.reuse.dot`. Earlier snapshot kinds
remain available; old logs without reuse data report that it must be recaptured.

## Historical Math six-round result (400e82b)

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

## Historical full-scan re-cut (72e9dc6, superseded below)

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


## Bounded adaptive implementation

The current runtime adds three independent mechanisms:

1. **Probation before installation.** A maximum of 128 exact candidate patterns,
   additionally limited to 8192 model units, retain first/last observation and
   opportunity credit. Least-recently observed entries are evicted when full.
   Repetition with enough estimated credit promotes a pattern into the stable
   dictionary; matching still precedes learning from the current event. Formal
   templates used by historical Uses remain immutable and are not evicted. The
   existing 4096-definition limit still applies. A fingerprint index stores IDs,
   not a second copy of full patterns; every hash hit checks complete equality.
2. **Affected-root cuts.** A reverse template-to-active-Use index and deduplicated
   FIFO dirty queue identify changed roots. Every 64 applies, process up to eight
   dirty templates. For each, recompute the cut only in its active roots, excluding
   that template; charge any newly exposed template once and remove the old shared
   charge once. Accept only strict improvement of selected wiring plus dictionary.
   Updates affecting over 256 roots are skipped and counted. This replaces the
   full-history scan and avoids starvation by low template IDs. It is bounded
   coordinate descent, not a global optimal cut or a generic dynamic graph solver.
3. **Local rotation.** Producer-suffix candidates may intersect part of an old Use.
   For example, `Use(A+B) + C` may become `A + Use(B+C)`. The old instance remains
   as history, A is explicitly restored as residual, and all references retain
   physical identities. Selection includes this spill cost. Dependency-convexity
   and full binding/effect verification remain mandatory. This is a bounded local
   boundary exchange, not enumeration of arbitrary tree rotations or subgraphs.

A structural hash collision test verifies that fingerprints never establish
pattern equality. Tests also cover bounded singleton traffic, profitable suffix
rotation with retained prefix facts, re-cut cost and owner/reverse-index integrity,
union evidence, online/offline/replay equality, and per-round DOT edge targets.
The existing debugger renders only the current cut, resolving stable ports through
its owners; retired parts are inspectable as history without being resurrected in
the overview. Typst still uses the existing Eggplant renderer.

### Complexity of the implemented path

Let M=16 be maximum region members; B the size of a member's typed binding/effects;
P<=128 probation entries; T<=4096 installed templates; F<=256 affected roots per
trial; K<=8 trials per maintenance call; and U retained historical Uses. Pattern
normalization and convexity retain their existing bounded searches. Fingerprint
lookup is expected O(MB) with a non-adversarial hash, but a collision bucket may
require O(TMB) comparisons. Probation's deliberately simple vector lookup costs
O(PMB); an eviction-heavy admission can additionally scan O(P^2 M) definition
lengths. These bounds make no assumption that hashing proves equivalence.

Each affected root has at most M members and its stored decomposition has O(M)
Use nodes. A cut trial costs O(FM log(FM+1) + FM log(U+1)), including memoization,
reverse-index updates, and owner changes. Temporary cut space is O(FM). K trials
run every 64 events. High-fanout updates are not secretly scanned: they are skipped
before collecting their roots. Stable-port resolution is O(1). This yields bounded
local maintenance with logarithmic ordered-index factors; it is **not** a claim of
O(log n) total analysis per event. Validation, candidate normalization, retained
history, per-round full snapshots and the other analysis passes still cost work.

Additional live metadata includes O(n) owner/residual records, O(UM) immutable Use
members/parts, O(TMB) installed patterns and O(PMB) probation. The new reverse index
is O(number of active Uses + T); dirty queue is O(T). Raw evidence and older-tier
projections remain resident. Whole-program memory does not become O(T).

### Reproduction

```sh
cargo build --release
python3 tools/reuse_ablation.py --history out/use-reuse-final/math/history.json --output out/adaptive-reuse-final --repeats 3
```

The output path must be new. First capture an eligible history with the regular
`analyze --recapture-tier0 --source ... --rounds 6 --save-history` command if needed.
The script records the history SHA256, four end-to-end policy arms, and three
interleaved eager/full replays with process timings and macOS peak RSS. It verifies
identical stats on repeated runs. These timings include other tiers and exports;
no tier0 execution takes place during this ablation. Final measured results are
in `adaptive-reuse-results.json`; old tables above belong to their stated commits.

### Final Math6 evidence (adaptive implementation)

| Policy | Installed templates | Selected wiring + active dictionary | Entire candidate index incl. probation | Cut visits | Rotations |
|---|---:|---:|---:|---:|---:|
| Eager, no re-cut/rotation | 3072 | 28901 | 391056 | 0 | 0 |
| Probation only | 145 | 27939 | 17566 | 0 | 0 |
| Probation + incremental cuts | 169 | 27854 | 19631 | 966 | 0 |
| Full, including rotation | 168 | 27169 | 19625 | 1035 | 19 |

The full policy improves selected-codec cost by 5.99% relative to eager, and
reduces candidate-index model cost by 94.98%. Probation alone has the lowest
whole candidate-index cost; adding cuts and rotations is not unconditionally
better on all objectives. Full selected wiring plus all candidate-index units is
44,372, still above raw wiring's 31,248. This is not net lossless compression of
the entire retained analysis state. The 23 maintenance passes visit only 1035
Use nodes, compared with 57,171 in the historical full-scan re-cut experiment;
the dictionaries differ, so this last comparison is not a fixed-library timing
ablation.

Three interleaved native history replays have median whole-flow time 2.216s eager
versus 1.779s full, and process peak RSS 1,014,087,680 versus 766,885,888 bytes.
These include snapshots/export, raw evidence, and Tier2 processing. Median Tier1
construction is 0.193s versus 0.200s: **no isolated builder speedup is shown**.
They are local diagnostic runs, not a statistical performance guarantee or
measurement of tier0 rule execution. Browser regression additionally validated
the real small `.egg` fixture, saved DOT equality, DOT/Typst rendering, download,
and the final Math6 directory in the existing tools page without page errors.
