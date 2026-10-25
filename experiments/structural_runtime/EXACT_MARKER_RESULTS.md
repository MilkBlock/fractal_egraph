# Shared exact condition markers and binding payloads

Implemented in the actual saved Raytrace Pass-3 runtime, behind `--exact-markers`.
The same ridge predictor, ordinary GPU matcher, joint closure and loop-context
services remain installed. This is **exact query reuse**, not a new RipSat closure
certificate. It is not a standalone matrix benchmark.

## Contract and execution

The admitted queries already have an important verified property: their anchor
binds every variable shared between atoms, and every residual atom has one bound
key. Local constants and repeated-variable equalities are checked by the typed
projection. Residual witness rows can therefore be represented as factors,
without inventing cross-row correlations.

For example, two consumers reading these conditions can share one marker:

```
consumer A: AnchorA(x,y), P(x,a,b), Q(y,c)
consumer B: AnchorB(y,x), Q(y,c), P(x,a,b)

marker: [(typed-P-projection, x-handle), (typed-Q-projection, y-handle)]
payload: [all complete P rows, all complete Q rows]
```

If P has rows `(x,1,10)` and `(x,2,20)`, its payload keeps those pairs. It never
creates `(x,1,20)` by combining independent column-presence bits. The two
consumers retain different root bindings and column maps. Multiple occurrences
of the same condition remain multiple factors: `P(x,a), P(x,b)` requires the
full product of the two witness choices, including equal choices.

The runtime now does the following:

1. Native committed anchor rows register actual uses of canonical condition sets.
   These identities contain typed projection IDs and exact binding handles;
   predicted feature similarity is irrelevant to identity.
2. Only a conjunction with at least two live consumers subscribes to updates.
   Losing sharing removes subscriptions and cached payloads. This is a sharing
   criterion, not an arbitrary budget or sample cap.
3. A singleton reuses the existing native projection slot's presence bit, version
   and whole-row answer buffer. It gets no duplicate condition table.
4. Multi-condition markers publish exact readiness after an entire commit batch.
   Deletion, replacement, subsume and union-induced canonical row replacements
   invalidate affected payloads. New sharing reads current facts, not an old
   readiness bit retained from a unique consumer.
5. At a Run boundary, the same marker is classified once for all eligible
   consumers. False markers avoid payload access. True markers retain factorized
   row buffers, stream each witness tuple once and scatter it into the consumers'
   complete native bindings. No expanded Cartesian cache is stored.
6. Every consumer keeps its original Run watermark. A tuple is emitted only when
   the maximum timestamp of its anchor and witness rows reaches that watermark.
   Existing native RHS ordering, function merges and rebuild remain unchanged.

Unshared conditions use the existing ridge/exact path. GPU prediction actually
ran in the final test: 829 new scores, 41 matrix banks. Exact markers are CPU
published and consumed in this revision; their truth plane has **not** been moved
into a new GPU matrix operator. No artificial unknowns are introduced just to
keep the predictor busy.

## Real workload result

The saved input, model, native kernel, GPU workers, joint/loop programs and native
control are unchanged and fingerprinted. The marker-off time is the most recent
applicable **historical** control, not a rerun or a same-binary paired ablation.
Each changed candidate was run once; all attempts remain recorded.

| Metric | Historical marker off | Final marker on |
|---|---:|---:|
| Whole Pass-3 inference | 16.316 s | 15.376 s |
| Provider maintenance | 0.900 s | 1.086 s |
| Provider queries | 0.527 s | 0.562 s |
| Per-root residual accesses, including cached answers | 708,814 | 472,053 |
| New condition-answer reads | 274,964 | 259,062 |
| Answer-cache probes | 523,073 | 400,206 |
| Witness rows materialized | 188,982 | 189,036 |
| Original eligible anchors in admitted queries | 691,325 | 691,325 |
| Complete bindings from admitted queries | 233,588 | 233,588 |
| All original rule bindings | 2,768,828 | 2,768,828 |

Residual accesses decrease **33.4%**, but fresh condition-answer reads decrease
only **5.8%**. Do not call the first number a 33.4% reduction in native Free Join
work or GPU candidate rows. Depth zero in the new histogram means marker routing
instead of the former residual loop, not free execution.

The marker route handles 235,999 candidate anchors, rejects 126,782 exactly, and
provides 142,562 full bindings. Those bindings are 61.0% of the admitted provider's
233,588 bindings, not 61.0% of the whole egraph. It streams 71,444 factor tuples for
162,404 consumer visits; output scatter and timestamp checks still run for each
consumer. The marker also performs 122,485 native leaf truth reads and 5,456
conjunction publication reads. These costs are explicitly reported.

Examples of avoided residual accesses (one original query each):

| Anchor | Exact residual condition | Avoided accesses | Bindings from marker |
|---|---|---:|---:|
| BodyContainsExpr | Single | 55,385 | 16,769 |
| Get | BodyContainsExpr | 39,066 | 50,036 |
| Get | ContextOf | 26,232 | 16,254 |
| Get | ExprIsValid | 15,060 | 0 |
| Get | ExprIsPure | 13,749 | 90 |

The inference sample is 5.8% faster than the historical control, but the provider
itself costs about 0.221 seconds more in these measurements. Unchanged native and
ordinary GPU phases also move. **Net acceleration attributable to this feature
is not established; it remains off by default.** This does not reach 6.17 seconds.

Earlier attempts explain the representation change. Maintaining all descriptors
performed 878,598 publication truth reads. Subscribing shared descriptors only
reduced this to 121,041. Reusing singleton native slots leaves only 5,456 reads
for multi-condition publication, plus the separately charged leaf reads above.
The first two full inference samples were 15.329 and 16.429 seconds respectively;
they are retained, not discarded to select the best result.

There are 113,672 native leaf descriptors and 173,455 conjunction descriptors at
the final snapshot, of which only 43,988 descriptors are shared. Multi-condition
subscriptions have 637 live factor connections. Unique descriptors still require
identity bookkeeping; this version does not claim those metadata are free or
that RSS has been reduced.

## Validation and limits

- 38 Rust tests pass: complete correlated rows, duplicate factors, reordered
  consumers, partial sharing, slot reuse, new sharing, late facts, function
  replacement, union, subsume and unchanged Run frontiers.
- 156,426 independent native audits of **provider-supplied queries** pass. This is
  not a fresh audit of every ordinary GPU fallback query.
- Final facts, function values, equivalence and visibility match the same-submit
  native control: 699,068 rows and 524,631 mapped eclasses.
- Actual Tiger extraction has equal selected DAGs for all 29 functions.
- Both backends retain the full original binding population, and ridge execution
  remains explicitly checked. No pristine/fused-native equivalence is newly claimed.

This uses the existing 68 admitted always-run queries. They exclude native
residuals and primitive guards, and enforce the anchor-complete contract. It does
not newly compile `Expr-size > k`; an Expr-size row's existence is not that guard.
Numerical values inside complete witnesses remain intact, and unsupported rules
keep their existing execution path. General multi-key spines and primitive-guard
contracts still need the corresponding compiler support.

## Reproduce or reuse

Use `build.py` with the same clean assisted-runtime checkout. Run the existing
`run.py` invocation from RESIDENT_RESULTS.md with `--resident-scope sort
--exact-markers --lanes gpu,audit --extract --compare-exact-markers CONTROL`.
Keep the same saved source, lowered model, ordinary query worker,
`--coordinates markers --joint-sat --boundary-programs PROGRAM --reuse-native
NATIVE_CONTROL`. The comparator verifies kernel/worker/model identities and
unchanged adapter sources outside `hook.rs` and `exact_markers.rs`; it labels a
reused historical runner explicitly. Repeating the same command reuses successful
lanes rather than rerunning inference.

The implementation is in `exact_markers.rs` and the existing Hook. No native
kernel files were modified. `exact-marker-results.json` includes all attempts,
source/binary fingerprints, all per-rule depth counters, marker publication and
payload work, graph and extraction checks. Final raw results are in
`out/exact-marker-final-pass3`. Redundant full exports from intermediate attempts
were removed to recover disk space; their reports, logs and checks remain.
