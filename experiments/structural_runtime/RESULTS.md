# Pass 3: exact completion works; current predictor has no net speedup

Measured once per lane on Apple M3 Max. Full saved raytrace Pass 3, original
schedule, stable choice and the existing common sorted native RHS protocol.
No rule effect, function value, equivalence or original table is ignored.
This is now a live runtime adapter, not frozen projected-query replay.

## Verification

* Seven rules selected automatically from compiled query shapes; 1,798 Runs
  per rule, **12,586 independent native incremental-binding audits** passed.
* Final **699,068 original table rows** agree, including values, equivalence
  and visibility. This row count includes analysis/function tables; it is not
  an Expr e-node count or a storage compression claim.
* Production Tiger selected the same DAGs for **29 functions**.
* Eight Rust tests passed, including the existing graph-comparison regressions.
  New tests force every proposal negative and cover late facts, witness
  products, union, aliases, subsume and function replacement.
* Three requests to the persistent matrix service: 888 reference scores,
  maximum absolute error `5.96e-8`, zero threshold disagreements. Model errors
  are permitted; CPU completion, not floating-point accuracy, ensures coverage.

## Does the CPU complement stay shallow?

Yes for this query class. Every shared variable is bound by a constructor
anchor, so the CPU can check residual existence before forming any witness
product. GPU scores only reorder these exact checks.

| Same affected anchors | Fixed CPU order | GPU-guided order |
|---|---:|---:|
| Anchor candidates | 25,247 | 25,247 |
| Exact residual-key reads | 39,290 | **33,509** |
| Mean residual checks per anchor | 1.556 | **1.327** |
| Complete bindings returned | 3,945 | **3,945** |

The read reduction is **14.71%**. These are the adapter's exact CPU probe
depths, not native Free Join candidate-row counts or the other GPU matcher's
Join depths. Per-rule populations, depth histograms, and equal-rule-weight
reach curves are in `results.json`; this is not a changed-rule-mixture claim.

Of 21,775 predicted-negative anchors, **77.24%** finish after one exact check;
**97.10%** finish within two. But CPU completion also recovers **2,594 valid
bindings**, versus 1,351 through the positive lane. Thus the static final-graph
holdout recall did **not** carry over to the live selected rules. The current
model often predicts impurity before/through the evolving analysis. No such
prediction is allowed to discard an anchor.

| Rule anchor | Fixed reads | Guided reads |
|---|---:|---:|
| Bop | 13,636 | 9,857 |
| Concat | 22,014 | 20,531 |
| Top | 3,097 | 2,528 |
| If | 417 | **467** |
| DoWhile | 84 | 84 |
| SubTuple | 32 | 32 |
| Uop | 10 | 10 |

The If regression is retained. Small/empty paths and normal indexing are not
awarded Cell proof credit; **no RipSat closure certificate is used here**.

## Full GPU runtime comparison

All unclaimed rules use the same existing full GPU matcher. No joint sat-skip
library is installed. Times include native RHS, merge/rebuild, event delivery,
CPU completion and predictor work inside the main inference interval.

| Lane | Inference |
|---|---:|
| Existing full GPU matcher, no new provider | **18.828 s** |
| Plus exact CPU completion, fixed order | 21.526 s |
| Plus matrix proposals and exact completion | **20.613 s** |
| Independent audited variant; diagnostic only | 22.255 s |

The matrix variant is **9.48% slower than the unchanged GPU baseline**. It stays
opt-in. It would be misleading to call fewer condition reads an end-to-end win.

The new provider's directly timed components are:

| Component | Fixed CPU | Matrix-guided |
|---|---:|---:|
| Commit/index/structural maintenance | 0.101 s | **0.980 s** |
| Query work, including features/routing/predictor roundtrip | 0.057 s | **0.256 s** |
| Predictor GPU device time, included above | — | 0.0052 s |
| Predictor roundtrip, included above | — | 0.0512 s |

There are only **499 new model/signature responses in 44 matrix requests**.
Traffic is 52,616 bytes up and 2,524 bytes down; this is no longer full JSON
graph transport. The remaining cost is mainly maintaining structural context,
not the matrix arithmetic. Union invalidation is still conservative: nearly
all of the 34,337 node-feature requests rebuild their local token signature.
Immutable signature predictions themselves are reused.

The ordinary matcher still executes **7,259 dispatches** in all three lanes.
Its device times also vary: 5.717 s baseline, 7.419 s fixed-CPU, 5.972 s guided.
Therefore the guided lane's lower total than fixed-CPU **cannot be assigned to
the learned ordering**: a larger difference occurs outside the new provider.
We have isolated a read-count improvement and measured added provider cost,
not explained every difference in the single-run wall clocks.

Setup is excluded and reported separately: 31.268/3.116/2.960 seconds for
baseline/fixed/guided, respectively. Shader/process/cache warmness differs;
these startup differences are not optimization credit. The historical same-
commit all-CPU control is 85.286 s and was reused for final-graph checks. It is
**not pristine egglog**, and its existing CPU/GPU ratio is not this feature's
speedup. A separate CPU-backed integration trial is kept in
`out/structural-runtime-pass3-v4`; it is not substituted for the GPU ablation.

## What to improve next

1. Publish compact feature/marker updates from commits, and invalidate only
   affected typed slots on union. Avoid global feature-cache clearing and
   repeated string/adjacency bookkeeping.
2. Target expensive query families whose shared structure removes substantial
   work. These seven rules have only two to four residual predicates; splitting
   them out does not remove a whole ordinary GPU dispatch.
3. Fit the proposal objective to live complete-candidate verification/complement
   cost. Current final-snapshot, per-key labels give weak live recall; they are
   not a reliable cost model for deciding which exact condition to check first.

None of those improvements justifies skipping the exact complement. The useful
new guarantee is that predictor quality affects cost, not the returned binding
set, within the checked anchor-complete query class.

## Failed development paths retained

The first event subscription unnecessarily included unrelated container-valued
constructors. Restricting it to the 17 constructor names actually read by the
feature program fixed this without dropping feature alternatives.

An earlier harness mixed sorted assisted submission with fused native execution
of other rules. Per-query audits passed but transient `Smaller` rows differed
by 60/61. A matching initialization clone alone did not fix it. Using the bench
skill's **full common submission protocol** restored strict final equality;
no original table was projected away. This does not establish equivalence to
arbitrary fused/native execution order. Failed artifacts remain in
`out/structural-runtime-pass3-v1`, `v2` and `v3`.

The successful full-GPU run is `out/structural-runtime-pass3-gpu`. Its source,
binary, worker, model identities and per-rule work records are in `results.json`.
