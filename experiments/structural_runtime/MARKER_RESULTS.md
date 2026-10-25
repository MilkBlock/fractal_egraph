# Commit events directly update compact context markers

Implemented in `markers.rs`, selected with `--coordinates markers`. This
replaces the context feature backend, keeping the original seven-rule library,
94-token vocabulary, model weights, GPU predictor, exact condition blocks and
native submission unchanged. The separate solver-generated `.egg` marker
pipeline is still not installed by this experiment.

## Representation

A typed value's visible feature state is **94 bits in two u64 words**. Queries
read those words and enumerate set bits for the unchanged GPU encoding. They
do not inspect constructor rows, traverse parents or reconstruct paths.

The compiler builds 94 counted unary channels and 12 necessary column-pair
projections. For example:

```
HeadConst(child)                       # provided by committed Const rows
E_Add_0(parent, child)                 # only this constructor-column projection
HasAddWithConst(parent) = [E_Add_0 * HeadConst > 0]
```

Two-edge features compose two such stages. Root, port, same-row alias and
return-alias features are published directly from the corresponding row event.
Projection multiplicities and feature support counts preserve disjunction:
removing one witness does not clear a feature supplied by another witness.

At each completed native commit, repeated deltas are coalesced. Updates use:

```
delta(E * m) = delta(E) * m_old + E_new * delta(m)
m = support_count > 0
```

The compiler orders channels by feature depth. Even if the original graph has
cycles, this finite feature program has no recursive channel dependency. A
simultaneous edge and child-marker update is accounted for exactly once. This
is currently a sparse integer program on CPU; we do not claim its updates are
GPU GEMMs. The existing numerical predictor still runs on GPU.

## What remains stored

There is **no general feature row arena or per-node adjacency index**, and no
complete feature-only table copy in the Hook. Original rows required for exact
CPU matching remain; they cannot be replaced by an approximate marker.

There are still:

* **137,153 distinct required projection links** in 12 families. These route
  later child changes to parents. They are connectivity information, not zero
  wiring or an excuse to claim the graph disappeared.
* **127,385 undo receipts**, normally a native input key plus its old result.
  Native journals provide the new row, so removals need enough information to
  undo the old contribution. An input override handles key/payload differences;
  this run needed zero overrides. Receipts are not per-rule histories.
* **417,171 support entries**, and **161,100 live feature bitmaps**. Counts are
  logical records, not RSS measurements or an egraph storage-compression ratio.

The important change is how the information is used: fixed delta operators
update feature bits during commits; later consumers only read marker state.
The projections do not retain all original-column correlations. Such
correlations remain in the exact matcher, and no feature bitmap is treated as
a complete rule binding or Cell-equivalence certificate.

## Same-binary Pass-3 comparison

Actual full saved raytrace Pass 3, stable choice and common sorted submission;
ordinary queries still use the existing GPU matcher. Each changed lane ran
once. Unchanged native and provider-off controls were reused explicitly.

| Same model and seven rules | Adjacency publisher | Direct markers |
|---|---:|---:|
| Commit maintenance | 0.260164 s | 0.316055 s |
| Query / coordinate preparation | 0.176051 s | **0.089596 s** |
| Exclusive provider total | 0.436215 s | **0.405650 s** |
| Full inference | **18.219959 s** | 18.285396 s |
| Ordinary GPU device time | 5.802019 s | 5.773399 s |

Query preparation drops **49.1%**; commit maintenance increases **21.5%** as
features are now materialized on write. The combined provider cost decreases
**7.0%**. Full inference is effectively unchanged in this single comparison
(the marker lane is 0.36% slower). This does **not** establish a net speedup.
The unchanged historical provider-off control is 17.713557 s; markers still
do not outperform it. The provider remains opt-in, and the representation
choice remains explicit for continued experiments.

Phase accounting was fixed: query time now excludes bootstrap ingestion,
which is charged to maintenance. These two current columns can be added.
Historical query/maintenance counters had one bootstrap overlap and are not
silently substituted into this comparison. GPU predictor/roundtrip time is
already included in query time and must not be added again.

This run processed 340,695 constructor notifications, coalesced 637,744 base
channel deltas and 389,779 link deltas, propagated along 263,992 links and
changed 997,827 feature bits. The resulting GPU request count is unchanged:
40 batches / 388 new model-signature scores. Exact condition-read and complete
binding counts also remain unchanged. Gains are representation work, not a
changed query population or extra Cell proof credit.

## Validation

* **16 tests passed**. New cases include multiple projected witnesses, late
  heads, self-cycles, deletion, table clears, key/payload differences, and 300
  batches of seven structural updates compared with an independent graph view.
* Real Pass 3: **21,770 marker/graph coordinate comparisons** and **12,586
  independent native complete-binding audits** passed.
* Final **699,068 original table rows**, values, equivalence and visibility
  match the native same-submission control. Production Tiger selects the same
  DAGs for **29 functions**.
* Audit deliberately keeps the old graph implementation as a reference; only
  the non-audit marker path has zero graph reads for coordinate lookup. The
  zero `general_adjacency_rows` / `query_graph_reads` counters describe the
  marker implementation, not the audit oracle or egglog's underlying graph.

## Reproduction and boundaries

Use the existing `build.py` / `run.py` workflow in README, adding
`--coordinates markers`. The run directory is
`out/structural-differential-markers`; same-binary adjacency control is
`out/structural-marker-control`. All tool/input identities and work counters
are retained in `marker-results.json`. No trace export or website deployment
is needed.

This implements direct runtime feature markers. It does not yet route the
solver-generated source markers into this program, create a `.egg` marker
sort, move native union/rebuild onto GPU, or remove the exact CPU complement.
