# Ripen preparation: target-local set ordering

Production `analyze` used to reject a composition if *any* set receipt had a
commit number smaller than the previous receipt in application order. Native
commit delivery across distinct targets does not imply a dependency between them.

Preparation now preserves monotonically increasing commit IDs per substituted
set target. It retains executable stage-local LHS checks, absence/old-value
checks, and committed-result checks. Different target expressions are **not**
assumed disjoint: if union aliases them, the guards execute against that shared
native state. This is validation of the extracted replay, not a universal proof
that arbitrary updates commute. Source action and application order are unchanged.

Regression cases cover cross-table delivery, distinct keys in one table,
reversed updates of one target, and falsified committed results.

```sh
cargo test --offline --lib native_analyze::ripen::entry::tests
cargo build --release --offline --bin egg_layout
EGG_LAYOUT_GRAPHS=0 target/release/egg_layout analyze \
  --replay-history /tmp/eggcc-set-final/live/history.json \
  --output out/eggcc-key-order
python3 experiments/ripen_replay_order/compare.py \
  /tmp/eggcc-set-final/live/analysis.json out/eggcc-key-order/analysis.json
```

The history is an existing native eggcc `fact` capture (one optimizer pass), not
part of this repository. Results compare the same 2,060 composition IDs. The
intermediate table-local ablation still rejected 1,194 combinations by order and
validated 620; table identity alone did not improve this workload.

A prepared composition is not a shareable cell. Container export and merge
certification remain separate gates. No Join saving, held-out cell coverage
increase, or runtime speedup is inferred from preparation counts.

## Measured result

| Preparation gate | Before | Target-local order |
| --- | ---: | ---: |
| Replayed, then blocked by export | 620 | 639 |
| Unique validated compositions | 550 | 569 |
| Commit-order rejection | 1,194 | 0 |
| Unresolved binding | 9 | 1,065 |
| Missing scalar or symbolic binding | 110 | 139 |
| Output alias mismatch | 87 | 177 |
| External injection failure | 40 | 40 |

Of the unresolved bindings, 1,056 are `then-case` and 9 are `inner`. They were
previously hidden behind the earlier commit-order check. The new code retains
those rejections rather than fabricating bindings. All 639 replayed compositions
still hit the container export gate; certified shared-cell count remains zero.
The gain is 19 validated compositions (+3.06% of the previous occurrence count),
not an established increase in held-out coverage. The frozen library is unchanged.
