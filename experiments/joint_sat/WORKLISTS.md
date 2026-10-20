# Transient binding worklists

Cell action sinks stage proven bindings in private Unit-valued tables. The
untraced path clears them after every native action batch. These are input
buffers, not public e-graph facts or a retained history.

The runtime now:

1. Merges only those input buffers when no unrelated table has pending writes;
   otherwise it preserves notification order and uses the ordinary merge path.
2. Temporarily excludes the buffers from union/container rebuild. Their values
   were canonicalized before the frozen action batch. No subsequent action may
   observe them until they have been emptied.
3. Clears each whole buffer, invalidates its indexes and keeps the original
   timestamp boundary. It does not individually delete every key or reset all
   unrelated table indexes during cleanup.
4. Restores the original rebuild mode on both normal and reported-error exits.
   The trace path retains its earlier rebuild/removal behavior.

Native RHS actions, public function merges, union and public rebuild are kept.
The low-level helpers are for exclusively owned, side-effect-free worklists;
they are not general permission to omit canonicalization of stored facts.

## Complete saved raytrace Pass 3

One release run per lane; compilation, setup, export and verification excluded.
This is the actual local egglog runtime, not pristine upstream or a simulator.

| Lane | Inference seconds |
|---|---:|
| Fused native | 25.112 |
| Same-submit CPU | 25.615 |
| GPU, sat-skip disabled | 19.519 |
| Cell-assisted joint path | 15.037 |

The same-run GPU-off/on ratio is **1.298x**. This is progress, not the requested
large final speedup. Both strict same-submit CPU/on and GPU-off/on public
row/value/equivalence/visibility gates pass. Production Tiger returns the same
byte-identical selected DAG in all three exported lanes (29 functions, 8,018
compared public rows), SHA256
`c48fe2b212d341b1c3f3de3ef94e77e3252b661e68be3bf60fbd417bb5ad3ce9`.

Evidence: `out/raytrace-scratch-clear-v1`. The preceding rebuild-only variant,
`out/raytrace-scratch-rebuild-v1`, was 15.579 s versus 18.678 s GPU-off and passed
strict graphs. These are separate single-run observations, not averages or a
statistical significance claim.

The regular benchmark skill runs the relevant runtime regressions. The bridge's
`cell_worklist_tests` additionally check foreign pending writes, public rebuild
while private rows are excluded, whole-buffer clearing and reuse.

The generic effect-prefix experiment was not adopted: it is preserved on
`codex/cell-effect-prefix-experiment`, commit `1093ae1`, with its negative
full-workload results. The default runtime does not include its additional
query predicates or prefix cursors.
