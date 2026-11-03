# Share complete multi-atom Spine prefixes

This extends `SHARED_CONTEXTS.md` in the standalone, single-thread Cell CPU
engine. It uses the same authenticated sum-bits Pass 3 source_sk_mk program and
merge programs. It does not change native egglog, GPU execution or RipSat closure
certification. Enable with both `CELL_CPU_RETAINED_CONTEXTS=1` and
`CELL_CPU_SPINE_PREFIXES=1`. Prefix sharing is **OFF by default**: maintenance work
decreased, but the final timing still regressed slightly.

## Representation and execution

The existing typed query IR supplies exact table identities, argument positions,
constants and repeated-variable constraints. The prefix compiler promotes the
existing structural anchor, then follows source atom order. It stops at a guard
or a disconnected read. Connectedness includes shared slots and fixed equality
identities. It normalizes variable names by first occurrence, never permutes
argument positions without a proof, and shares prefixes used by multiple rules.
There is no fixed prefix-length, candidate or memory budget.

Shared prefixes form a DAG. A longer prefix extends its longest shared ancestor;
it does not acquire the ancestor's facts a second time. Each consumer attaches to
its longest shared prefix, with explicit slot and witness projections back into
the original query. This is exact common-subexpression elimination for connected
table prefixes, **not arbitrary common-subgraph mining or guard specialization**.

For example six actual always-run rules share:

```
BodyContainsExpr(body, expr)
DoWhile(inputs, body)
```

The shared result includes `body, expr, inputs, loop`, both physical fact
witnesses, all query output coordinates, and their newest epoch. If several loops
share a body, their input/loop pairings remain separate. Consumers continue with
their own Get/Concat/Single/etc. reads and guards. A single existence bit would
not preserve these bindings.

The runtime reuses the existing exact retained matcher for both providers and
suffixes. Only the query trees and correlated results are shared; RHS submission
still belongs to the original source Run.

- Providers subscribe to committed row changes through the existing exact read
  domains. Union, function replacement and hidden/deleted facts invalidate the
  affected paths. Fixed equality literals also refresh after union, including
  when no old matching row contained the displaced identity.
- Consumers retain their own suffix state and semi-naive watermark. One consumer
  refreshing a provider does not consume another consumer's delta. Naive rules
  still emit all current bindings on every allowed Run.
- Provider results preserve complete correlated coordinates. If several proofs
  yield the same coordinates, keep a valid proof with the newest witness epoch;
  an old proof must not hide newly published evidence from a later consumer.
- Unchanged records share immutable payloads. Consumers index their state by a
  provider-local binding number, rather than copying a full parameter vector as
  another map key. These numbers are **not graph/eclass IDs**. Removed numbers
  are never reused; the current identity map retains no history of removed rows.
  A delayed consumer cannot mistake a new insertion for its retired binding.
- Removed seeds retire their suffix trees and subscriptions on the consumer's
  next legal Run. Providers keep current results, not per-stage history. Slow
  consumers may retain old payloads until they run; this is not asynchronous GC.
- Checkpoints preserve the authoritative graph and watermarks. Provider/suffix
  caches rebuild cold after resume without replaying old effects.

No future RHS is exposed, no merge is reordered, and no existing constructor or
union effect is omitted. This is not the unfinished cross-ruleset RHS-completion
feature or a new certified Cell endpoint.

## Sum-bits Pass 3 measurement

Final raw output is `out/spine-prefix-identities`. Each lane ran once, with no
builds/tests/audits overlapping timing. Audits ran afterward. The previous two
source revisions and their regressions are preserved in `spine_prefix_results.json`;
do not select the fastest lane from another revision or average these attempts.

| Metric | Ordinary matching | Shared individual reads | Shared multi-atom prefixes |
| --- | ---: | ---: | ---: |
| Inference | **127.024 ms** | 129.080 ms | 130.784 ms |
| Setup | 1.135 ms | **1.095 ms** | 1.517 ms |
| Peak RSS | **201.84 MiB** | 210.22 MiB | 209.84 MiB |
| Candidate checks, maintenance included | 84,033 | 61,145 | **61,135** |
| Query row visits / additional context decoding | 113,887 | 76,818 | **76,794** |
| Exact-key lookups | 62,212 | 55,369 | **55,365** |
| Live context nodes, signal headers included | — | 9,640 | **8,802** |
| Query-table branches | — | 9,046 | **7,733** |
| Shared-prefix → suffix links | — | 0 | 467 |
| Contexts allocated across the run | — | 14,602 | **13,343** |
| Row-to-context notifications | — | 19,630 | **17,563** |
| Context invalidations | — | 5,832 | **5,391** |

The new layout has 7,733 query-table branches **plus 467 prefix links**, or 8,200
such links versus 9,046 previously. Do not count only the first category and claim
all missing links disappeared. These counters describe query contexts, not Cell
instance count or physical memory. RSS is measured separately.

The final single-run result is **1.32% slower than shared reads**, **2.96% slower
than ordinary matching**. No net acceleration has been demonstrated. Search is
64.758 → 64.687 ms; index/context publication is 9.460 → 10.005 ms. Other unchanged
phases also vary; the full elapsed difference is not causally attributed to one
new data structure.

The bounded conclusion is that duplicated tree maintenance was removed, but
candidate checks barely decreased because the previous implementation already
shared exact reads. Current prefix-result collection and consumer routing still
cost work. The final reduction in maintenance counts does not justify enabling
the feature globally.

## Coverage and attribution

The compiler finds **14 multi-atom prefixes for 31 of 216 selected rules**. Eight
providers receive requests and have final results in this sample. The two nested
three-atom provider programs do not run here; dedicated native-backed fixtures
exercise nested prefixes up to four atoms.

Runtime counters:

- 454 consumer calls, 420 emitted complete bindings out of 5,271 retained outputs;
- 454 provider requests, 97 refreshes, 357 reuse calls;
- **258 reuse calls from a different consumer**; ordinary self-revisits are not
  credited as cross-rule sharing;
- 172 current provider bindings; 607 prefix-to-suffix attachments and 140 removals;
- 2,079 result rows collected across refreshes, 854 suffix updates;
- 25 refreshes kept identical output and therefore did not advance the version;
- provider maintenance/request work totals 2.072 ms inside search, not an isolated
  estimate of savings.

For the six-rule BodyContainsExpr/DoWhile provider specifically: 138 requests,
26 refreshes, 112 cross-consumer reuses, 41 current correlated bindings. No
new predicate selectivity or impossible-match proof is claimed from these hits.

The next limitation is structural coverage: this compiler shares connected
prefixes under the current anchor/source order. It does not factor an identical
internal fragment out of differently ordered queries. Expanding that scope must
also measure intermediate-result size; forcing a larger common join before a
selective consumer condition can increase work.

## Validation and reproduction

The final implementation passes all 85 Python tests and 7 Rust unit tests.
The binding-ID path also passed the focused prefix/retained checks. Native-backed tests cover renamed slots, repeated
variables, constants, nested prefixes, late inputs, multi-valued union results,
subsume/delete, function replacement, forced fingerprint collisions, index reset,
finite Run isolation, naive replay, and checkpoint restore. A deleted binding
reappearing before a slow consumer runs has its own regression test.

All **1,497 snapshots** match shared-read execution: complete facts, function
values, equivalence, visibility, binding lists, Run paths and iterations. All
three lanes have identical final state and Run controls, and a fresh prefix-mode
process reproduces its exact final state: **4,314 records / 2,324 eclasses**.
There are still 19,188 constructor requests, 3,210 union requests, 1,039 effective
unions and 951 repair collisions in every lane. This comparison does not resolve
the earlier raw-native full-Pass-3 discrepancy.

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml \
  --bin eggfuse-cell-runtime --example project_stages --example project_semantics

EGGFUSE_TEST_FRONTEND=/tmp/eggfuse-ripen-build/debug/examples/eggfuse_frontend \
CELL_CPU_SPINE_PREFIXES=1 /opt/homebrew/bin/python3 -B -m unittest \
  experiments.cell_runtime.prefix_test experiments.cell_runtime.retained_test

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --compiled out/local-sat-connected-compile --feature spine-prefix \
  --prefix-ablation --output out/NEW --audit
```
