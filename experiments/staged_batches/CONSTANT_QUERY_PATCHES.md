# Separate index order from binding order

The old query patcher preserves row order inside a hash bucket by walking from
its head for each middle insertion. Many equal values can make a set of updates
quadratic. The resident experiment unlinks and prepends in O(1) per changed
output/timestamp entry instead.

To preserve effect order, query output carries its actual nested-loop row
coordinates (and effective delta timestamp where applicable). The existing
binding sort compares these coordinates before its ordinal tie break. It does
not use atomic arrival order or the new bucket traversal order as semantics.
Exact keys/guards and complete binding enumeration are unchanged. Legacy split
matching retains its old ABI; unordered patches require witness ordering.

Reproduce the actual resident/native differential test:

```sh
python3 experiments/staged_batches/check_query_patches.py \
  --build out/eggfuse-clean-paths-build --output out/constant-patch-verified --rows 8192
```

8,192 inputs, 4,096 updates into a shared value bucket:
**1.127941 -> 0.108794 seconds**. All final rows, IDs, equivalence, values,
visibility and per-Run history agree; both variants independently pass native
comparison. This is one paired microbenchmark, not a full Pass 3 speedup.

The 19 general resident fixtures also pass with unordered patches and explicit
witness order. A full Pass 3 run is recorded separately; until it completes this
path is an explicit `ordered_query_patch=False` experiment.

Related finding: the old Spine-cache reuse cursor incorrectly expected descending
fact IDs, while both current index builders enumerate ascending physical rows.
The corrected cursor reuses 296,727 stable tuples in the one-changing-port test,
but that serial cache still takes 1.127922 s versus 0.060861 s for direct parallel
queries. The normal resident path therefore bypasses this cache while retaining
the entire Analyze/Bake library and every Cell recipe. This is not a reduced
library or a match-coverage budget.
