# Share exact input domains before continuing individual rules

This extends `RETAINED_CONTEXTS.md` in the **standalone single-thread Cell CPU
engine**. It does not modify native egglog or the compiler's Bake coverage. The
input is the same authenticated sum-bits Pass 3 `source_sk_mk` artifact, merge
programs ON. The old invalid Bake artifact is not used.

Retained matching remains opt-in (`CELL_CPU_RETAINED_CONTEXTS=1`). Within that
mode shared domains are now the default. `CELL_CPU_RETAINED_SHARED=0` selects the
previous independent per-rule domains for ablation. The target rulesets remain
`always-run,type-analysis`. No size limit, eviction policy or candidate heuristic
was introduced. Ordinary source Run visibility and RHS order are unchanged.

## What is shared

The 216 selected rules have 63 alpha-normalized structural anchor shapes. For
example 17 start from `Bop` and 12 from `Concat`. This static overlap is motivation,
not a speedup measurement.

A shared read domain is keyed by **table identity, every bound column value, and
equalities between repeated unbound columns**. Table identity fixes the types.
Caller slot numbers are absent from the key. Thus rules with different variable
names can reuse a domain, while `R(x,x)`, `R(x,y)`, different constants and different
bound arguments remain distinct. Results contain exact row values, witnesses and
publication epochs, not fingerprints used as proofs.

1. The first consumer acquires the complete visible domain. Consumers retain
   their own correlated query branches but share immutable row payloads.
2. At the end of a source Run, the ordinary index update already decodes repaired
   rows. It moves these payloads into a transient publication batch. Duplicate
   old/new range notifications are removed when the indexed key is unchanged.
3. Each subscribed exact predicate evaluates a changed row once. Only an actual
   membership, value or epoch change notifies its consumers. A broad index-bucket
   collision alone no longer causes a context wakeup.
4. The consumer applies changed witnesses to its sorted branches in place. It
   no longer rebuilds a BTreeMap of every old branch for a small delta. Unchanged
   bindings also bypass repeated domain-key construction.
5. When the last consumer retires or rebinds, the domain and its subscription are
   released. Private domain/context IDs are recycled. Deferred consumers retain
   pending changed witnesses until their legal Run; no asynchronous-GC guarantee
   is claimed. Publication payloads are drained after the batch.

Different rulesets may read the same domain at different times. Each still uses
its own semi-naive watermark, complete binding projection and allowed Run. No RHS
executes in the publication callback. Multiple alternatives are never combined
column-by-column. Union, fixed equality literals, index reset, hidden/deleted rows
and function replacement retain the conservative invalidation from the previous
implementation. No graph-ID scheme or merge semantics changed.

**This shares exact table-read/filter computations under known bindings, not a
whole multi-atom Spine execution node or a certified RipSat closure.** The full
per-rule continuation trees are still separate. Ordinary index reuse is not
credited as new Cell closure inference. The unresolved custom-merge closure
certification and older raw-native Pass 3 differences remain unresolved.

## Final same-binary measurement

Each lane was timed once, without concurrent builds/tests/audits. Historical
intermediate revisions are preserved in `shared_context_results.json`; they are
not averaged or selected as best-of runs. Raw final data: `out/retained-shared-final`.

| Metric | Ordinary matching | Independent contexts | Shared contexts |
| --- | ---: | ---: | ---: |
| Inference | 126.642 ms | 132.516 ms | **123.804 ms** |
| Total search | 68.485 ms | 70.206 ms | **61.222 ms** |
| always-run search | 20.345 ms | 21.760 ms | **15.447 ms** |
| type-analysis search | 7.142 ms | 6.168 ms | **4.221 ms** |
| Index/context publication | **4.583 ms** | 7.878 ms | 8.941 ms |
| Candidate predicate checks, maintenance included | 84,033 | 73,987 | **61,145** |
| Query row visits, including additional context decoding | 113,887 | 98,929 | **76,818** |
| Exact-key lookups | 62,212 | 57,633 | **55,369** |
| Peak RSS | **201.20 MiB** | 210.31 MiB | 211.09 MiB |

The final single-run net improvement is **2.24% over ordinary matching**, **6.57%
over independent contexts**. This is a small observation, not evidence of a
stable large speedup. The feature remains opt-in. Search saved 7.263 ms, while
publication added 4.358 ms. Other phase differences are not separately credited.
No reduction in RHS constructor/union work is claimed.

There are 2,313 live shared domains serving 6,526 consumer subscriptions, with
3,002 current matched rows. Maximum fanout is 234 consumers; these are query
contexts, **not 234 certified Cells**. The per-rule trees still have 9,640 nodes
and 9,046 branches. Sharing lowered duplicated work but did not lower RSS here.

Within retained matching, exact row checks fell 24,263 → 11,421. The latter
includes **9,269 event-time predicate checks**, even if the consumer never runs
again, so moving work into maintenance is not hidden. Of those, 2,281 changes did
not change the exact result and avoided consumer notification. Notifications fell
22,146 → 19,630; dirty contexts fell 6,954 → 5,832. Publication reused 4,866 rows
already decoded by index maintenance and performed **zero additional event-row
decodes**. Query indexes still have their own separately reported decoding work.
`rows_by_rule` describes acquisition-time work; shared publication work cannot be
assigned exclusively to one rule and is reported separately.

## Verification and remaining priority

- 75 Python tests and 7 Rust unit tests pass; final retained-specific suite has
  18 tests, including its loop over 52 existing semantic fixtures.
- Added native-backed tests for cross-rule/Stage sharing with renamed slots,
  repeated-variable equality, literals, missing inputs, replacement, deletion,
  and forced hash collisions; the independent-domain ablation remains checked.
- All **1,497 snapshots** match ordinary execution: complete graph, functions,
  equalities, visibility, bindings and schedule. All three timing lanes have
  identical Run controls and final state. A fresh shared-mode process reproduces
  the exact final state: **4,314 records / 2,324 eclasses**.

The next structural opportunity is to share multi-read prefixes, rather than
retaining the same upstream bindings in several query trees. It must preserve
consumer projections, independently arriving inputs and stage watermarks. Before
expanding to cross-ruleset RHS completion, the remaining 4.36 ms publication
premium needs to be justified or reduced. Larger Bake libraries alone do not
establish that benefit.

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml \
  --bin eggfuse-cell-runtime --example project_stages --example project_semantics

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --compiled out/local-sat-connected-compile --feature retained \
  --shared-ablation --output out/NEW --audit
```
