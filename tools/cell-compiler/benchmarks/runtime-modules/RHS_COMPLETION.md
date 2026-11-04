# Switching to a previously supplied constructor result

The iterative compiler now derives a finite RHS program for each matched prefix of an admitted block. The two execution arms are eligible for exactly the same consumer Run. The ordinary arm uses the original effect instructions. The specialized arm uses checked producer receipts, current anchor parameters and the original effect order.

## Compiler proof and runtime boundary

The compiler symbolically evaluates the expanded bounded path. It propagates the actual anchor-column mapping, literals, repeated slots, SSA copies, constructor key/result relations and earlier committed equalities. Queued union facts become available after the producer boundary. A mutable function's output is fresh at the next Run. A matching constructor key can select a query fact or effect of an earlier step. The proof records the donor rule, source position, primitive-step distance and consumer instruction. A constructor set can also be marked as already equal. The artifact contains the switch table, fallback suffixes, exported operands and the whole-RHS preflight decision. Loading independently reconstructs these from the typed recipe and refuses altered instructions.

A cursor exports only requested weak references, with a bounded history. The current row must still exist, be visible, have the right table and complete canonical key, and be outside a dirty constructor table. Every accepted constructor read supplies its original fact to subsequent publication. For a set, the original union is still queued unless the compiler proved the equality and the values currently agree. Unknown donors, changes to the anchor result, deletion or dirty keys use ordinary effects as needed.

A fully supplied RHS can execute without registering a consumer frame. The executor checks every operand first. Only afterwards does it reserve the original Cell ID, enqueue the remaining unions in order, and publish keys and receipts at the original barrier. Failure before commit leaves the normal path free to execute the rule once. The focused regression reads two constructor results from two Runs back. One sibling is deleted between stages. It checks a successful closure, an atomic fallback, reconstruction of the missing row, and the original schedule/graph/bindings.

Every checked leaf with complete entry coverage now passes the sealed tuple and winning cursor directly to RHS application, even when the leaf is outside a longer macro. Partial coverage keeps the ordinary origin lookup. The compiled switch keeps suffix transitions so accepting a longer learned unit cannot hide a usable shorter suffix at the same boundary. Deployment follows the precomputed transition on the publishing operation and checked consumer entry, carrying the switch state instead of rebuilding a training segmentation. Training additionally counts adjacent non-overlapping witnessed operands. With the `rhs-lookups` objective, admission ranks additional provable constructor key reads by observed support before the existing origin-copy score. A bridge can also enter the bounded candidate set if both adjacent pairs were observed and extending them exposes a donor from the first operand. Its score is a proposal only. The joined bridge must actually execute and meet that consumer in the next round. No origin-copy credit is assigned to training a macro when the leaf already has the direct handoff. Predicted opportunities are separate from successful runtime receipts. Registering a native Cell for this mode also records its verified terminal program. None of these rules authorize eager execution of a later Run.

## Real Pass 3 programs

The control is the single-threaded release at `508e1c8`, using the frozen 64-block artifacts. **Dispatch** compiles the frozen transitions and direct binding path with result reuse disabled. **Fixed** specializes those exact catalog entries. **Ranked** starts with the same 32 leaf blocks and relearns up to 8 adjacent units in each of four rounds on the respective workload, with the original 64-unit and four-stage limits. This measures within-workload specialization. The optional catalog field enables the new execution path.

| Target | Catalog | Constructor requests avoided | Constructor set lookups reused | No-op unions omitted | Complete RHS frames avoided | Graph key lookups |
|---|---|---:|---:|---:|---:|---:|
| sum-bits | base | 0 | 0 | 0 | 0 | 20,088 |
| sum-bits | dispatch | 0 | 0 | 0 | 0 | 20,088 |
| sum-bits | fixed | 272 | 165 | 165 | 165 | 19,651 |
| sum-bits | ranked | 272 | 165 | 165 | 165 | 19,651 |
| raytrace | base | 0 | 0 | 0 | 0 | 5,647,088 |
| raytrace | dispatch | 0 | 0 | 0 | 0 | 5,647,088 |
| raytrace | fixed | 30,896 | 17,338 | 17,338 | 17,152 | 5,598,854 |
| raytrace | ranked | 30,900 | 17,346 | 17,346 | 17,152 | 5,598,842 |

One Raytrace example is producer rule 143 publishing `SubTuple(t, i, 1)`. Its query witnessed `Get(t, i)`, and its RHS constructed `Single(Get(t, i))` and equated it with the published result. Rule 137 on this entry needs the same `Get` and performs a constructor set on the same `Single` key. A successful whole-RHS preflight forwards both receipts, supplies the result slot and suppresses the already-equal union. Both real workloads use one-step donor distances in their learned programs. Longer learned execution paths and deeper physical donors have distinct counts.

| Target | Catalog | Units | Inference median | Paired inference Δ vs #29 | CLI median | Faster inference pairs |
|---|---|---:|---:|---:|---:|---:|
| sum-bits | base | 64 | 34.072 ms | +0.000 ms | 133.144 ms | — |
| sum-bits | dispatch | 64 | 34.156 ms | -0.026 ms | 132.512 ms | 4/8 |
| sum-bits | fixed | 64 | 33.937 ms | +0.090 ms | 132.846 ms | 4/8 |
| sum-bits | ranked | 37 | 33.741 ms | -0.118 ms | 134.439 ms | 4/8 |
| raytrace | base | 64 | 15.461 s | +0.000 s | 19.136 s | — |
| raytrace | dispatch | 64 | 15.139 s | -0.679 s | 18.788 s | 3/4 |
| raytrace | fixed | 64 | 15.049 s | -0.645 s | 19.463 s | 3/4 |
| raytrace | ranked | 39 | 15.800 s | -0.209 s | 19.131 s | 2/4 |

For Raytrace, making the binding handoff available to every fully covered block entry increases direct applied origins from 61,407 to 538,930. With the existing 64 blocks, the combined path reduces the inference median from 15.461 to 15.049 s. Its paired median difference is −0.645 s, faster in three of four blocks. Against dispatch alone, the additional RHS specialization is faster in only one of four blocks, with a paired median of +0.213 s. Sum-bits fixed versus base changes direction between blocks. Whole CLI time also changes direction across modes.

The lookup-driven catalogs stop at 37 and 39 units. Raytrace saves 48,246 key lookups and avoids 17,152 consumer frames, but ranking saves just 12 more key lookups than specializing the existing catalog. Its separately measured median is 15.800 s. The operation savings are deterministic on these inputs. These samples do not establish a throughput gain from re-ranking or RHS substitution alone. The new switch remains opt-in.

The table reports fresh-process release medians. Each four-block cycle uses balanced ordering. Sum-bits has eight measured blocks and Raytrace has four, following an excluded warm-up for each lane. The paired difference is computed within each measurement block before taking the median. CLI time also includes loading, initialization, export and report writing. Training, compilation and semantic audit are outside the measurements. Separate audited runs compare all ordered RHS digest bytes and the complete winning cursor-prefix tree against the frozen references on 1,428 and 16,883 Runs. All timed and audited trials check the final logical graph, schedule digest, resumption count, maximum depth, logical bindings, facts, function updates and effective unions. [Raw timings, counts and round-by-round training](rhs-completion-results.json)

## Reproduction

Use the original 32- and 64-block artifacts from the [iterative Bake study](ITERATIVE_BAKE.md). Use a new path for each result.

```sh
m=engine/cell-runtime/Cargo.toml
export CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 CELL_CPU_FREE_JOIN=planned
export CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1
export CELL_CPU_KNOWN_RESULTS=static CELL_CPU_RANGE_GUARDS=1 CELL_CPU_SLOT_TRANSFER=off
export CELL_CPU_FD_KEYS=0 CELL_CPU_SCALAR_MERGE=0
cargo run --release --locked --manifest-path "$m" --features iterative-bake \
  --example complete_blocks -- "$seed64" fixed.json
cargo run --release --locked --manifest-path "$m" --features iterative-bake \
  --example complete_blocks -- "$seed64" dispatch-only.json --dispatch-only
cargo cell-iterate --program "$seed32" --output ranked --rounds 4 --objective rhs-lookups
cargo run --release --locked --manifest-path "$m" --features iterative-bake -- \
  --program ranked/program.json --output timed-ranked
cargo run --release --locked --manifest-path "$m" --features iterative-bake,spine-shapes,slot-transfer-audit -- \
  --program ranked/program.json --output checked-ranked
cmp checked-ranked/rhs.json "$frozen_rhs"
```
