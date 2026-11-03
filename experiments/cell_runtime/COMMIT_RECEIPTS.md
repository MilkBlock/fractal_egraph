# Direct commit receipts for Spine context capture

This extends [entry coverage](STAGE_ENTRY_COVERAGE.md) in the standalone,
single-thread Cell CPU executor. It removes duplicate key lookup when retaining
an already matched or executed port. It uses the existing typed query/action
program and names no application operator, table or ruleset.

Enable with `CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1`. The receipt
flag is OFF by default: the final real-input pair removes nearly all capture
lookups, but does not demonstrate a net inference speedup. Merge programs stay
ON. This does not link native egglog, run on GPU, change source scheduling or
certify terminal Cell substitution.

## Information passed by the query and action sinks

Previously capture reconstructed each query/RHS row from recipe parameters.
After normal repair it used `(table, canonical key)` to look up its witness.
The same constructor or function had already been found during matching or
RHS execution.

The executor now carries an optional `Dependency` receipt alongside each
captured port:

- LHS matching records exact table witnesses independently of storage-provenance
  `MatchedFacts`. Ordinary matching, merge-point entries, composed entries,
  cached query replay, retained contexts and stage-Spine answers carry these
  witnesses through the complete binding. Storage ownership/absorption still
  receives its original witness representation.
- Construct instructions return the reference actually reused by hashcons, or
  the reference just activated. Their public value-returning API remains
  available for callers that do not request receipts.
- Virtual table sets return their actual existing/new fact reference. A proposed
  constructor output may still enqueue union; it does not replace the committed
  row witness.
- Function sets return the actual function-port index from the existing ordered
  update/merge operation, including unchanged outputs. Function reads retain
  the witness from their already-required lookup. Capture later follows the
  live port and therefore observes the actual merge result.

No original constructor request, function update, custom merge or union is
skipped. Receipt collection does not publish a future RHS. Pending ports become
available to later stages only after the original Run's repair and absorption.

## Same-key forwarding and validation

A raw physical handle may move or disappear before publication. When receipts
are enabled, the runtime records transient forwarding for:

1. Global constructor repair proving identical canonical keys.
2. Constructor duplicates proved by the existing local congruence path.
3. Function-port coalescence at identical canonical keys after union.

Existing exact storage-relocation forwarding is followed as well. Sharing an
eclass result alone never authorizes forwarding one constructor occurrence to
another. Deletes are not coalescence and do not receive such a link.

The proof for a direct receipt is:

```
original query/action established this exact table and input key
    + immutable captured parameters, canonicalized after union
    + forwarding that preserves the same canonical key/table
    + final live, visible witness
    => current port can be retained without another key lookup
```

Normal execution checks current table, liveness and visibility through metadata.
It does not decode the fact a second time. In executor audit mode, every accepted
uncached receipt additionally decodes its full current row and checks the table,
canonical key and visibility. A key mismatch fails the audit rather than being
accepted as evidence. The full-stage streamer enables this audit through its
existing binding-audit mode.

Missing receipts, deleted/hidden references and other failed metadata checks
use the existing exact keyed fallback. Instruction/value-only reuse and local
successor bindings that did not obtain physical witnesses retain that fallback.
These are not invented constructor witnesses. Custom-merge internal constructors
continue through their existing ordinary event paths; this capture mechanism
covers original query and RHS ports, including their actual merged function port.

Forwarding exists only for the current Run boundary and is cleared after
publication. It is not serialized into checkpoints or retained as historical
lineage. Checkpoints restore the authoritative graph/watermarks and rebuild
optional Spine contexts cold.

## Real sum-bits Pass 3 measurement

Final raw output: `out/stage-spine-receipts-final`. The same binary executes the
same authenticated source_sk_mk artifact with stage Spine, partial coverage and
shared entry validation fixed ON. Only `CELL_CPU_STAGE_SPINE_RECEIPTS` changes.
Each lane ran once, without builds/tests/audits overlapping timing. Full-stage
and key audits ran afterward. Data and fingerprints, including the superseded
full-row-validation draft, are in `commit_receipt_results.json`.

| Metric | Keyed capture | Direct receipts |
| --- | ---: | ---: |
| Capture key lookups | 20,129 | **58** |
| Total exact-key lookups, all runtime phases | 81,412 | **61,341** |
| Extra full-row receipt decodes in timing | 0 | 0 |
| Inclusive candidate checks | 34,279 | 34,279 |
| Query row decodes | 59,961 | 59,961 |
| Current continuation port refreshes | 6,208 | 6,208 |
| Capture time | 7.315 ms | 7.384 ms |
| Spine publication time | 18.052 ms | **14.825 ms** |
| Inference | **159.825 ms** | 160.522 ms |
| Peak RSS | **205.48 MiB** | 205.73 MiB |

Capture lookups fall **99.71%**, and total key lookups fall **24.65%**. Of 20,129
unique-key receipt attempts, 20,071 pass directly: **11,389** from query witnesses
and **8,682** from RHS effects/reads. There are **834 receipts whose handles
change** while following forwarding; this counts receipts, not forwarding edges
or union-lineage depth. All 58 rejections retain exact fallback. The batch
resolution map still shares results across repeated capture keys.

These are port-resolution counts, not new mutations or avoided candidates.
The established **34,279 inclusive candidate checks are unchanged**. This change
removes capture lookup work; it does not add a new query refutation or avoid an
inner join. The existing 17,330-inner-row depth study in the preceding report is
not presented as a new measurement for this binary.

Publication improves 3.228 ms in this pair, but whole inference is **0.44%
slower**. Witness-vector collection, parameter reconstruction, canonicalization
and reference maintenance still have costs. Differences in unchanged phases
are not attributed causally from one sample, and no net speedup is claimed.

The first draft validated every receipt by decoding a row during normal
publication. It also removed all but 58 lookups, but inference was 159.135 ->
169.220 ms (6.34% slower). That is a different source/binary revision, retained
as an intermediate attempt, not another sample to average or select from.

## Correctness and scope

All **1,497 exact stage snapshots** agree OFF/ON, including every auxiliary table,
function value, equivalence, visibility, full binding list, Run path, iteration
and changed flag. Accepted direct receipts also pass full canonical key audits.
A fresh process exactly reproduces the ON state.

Both lanes retain 12,500 logical bindings, 19,188 constructor requests, 3,210
union requests / 1,039 effective unions, 951 repair collisions and 1,496 source
Run/repair calls. Endpoint: **4,314 rows / 2,324 eclasses**. The 3,775 resumed
Spine bindings and 12-step maximum path are unchanged. These checks compare
standalone Cell executions; the prior raw-native whole-Pass-3 discrepancy and
terminal closure limitations remain separate.

- **21 receipt tests pass**, including inherited native-backed/semantic fixtures,
  new/reused facts, actual custom-merge outputs, constructor/function coalescence,
  deleted/recreated function ports, hidden evidence, instruction-only reuse,
  original-stage isolation and receipt-enabled cold checkpoint resume.
- Runtime discovery: **223/224 pass**. The preexisting
  `ReferenceTests.test_marker_multiple_owners_union_late_consumer` still expects
  `distinct_delivery_events < delivery_events` with values `16 < 16`. Graph and
  binding comparisons preceding it pass; the prior work reproduced the same
  failure on its unmodified parent. The assertion is left unchanged.
- **7 Rust unit tests and 13 compiler/IR tests pass.**

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml \
  --bin eggfuse-cell-runtime --example project_stages --example project_semantics

EGGFUSE_TEST_FRONTEND=/tmp/eggfuse-ripen-build/debug/examples/eggfuse_frontend \
  /opt/homebrew/bin/python3 -B -m unittest experiments.cell_runtime.stage_receipts_test

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --compiled out/stage-spine-compile --feature stage-receipts \
  --output out/NEW --audit
```

No retraining, source-name special case or artifact schema change is required.
The standard typed program validator still reconstructs the execution contract;
these receipts are internal runtime evidence, not trusted serialized claims.
