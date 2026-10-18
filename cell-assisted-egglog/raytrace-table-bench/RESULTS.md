# Real raytrace search benchmark

One release measurement, 2026-10-18, Apple M3 Max, one native worker thread. No averages. Same current native fork, not pristine. Original input, native checkpoint and executable hashes are in `results.json`.

## Original 14-rule saturation packet

| Component | Seconds |
|---|---:|
| Packet wall | 7.940515 |
| Search + apply (includes RHS staging) | 6.775248 |
| Insert + merge commits | 0.049361 |
| Rebuild | 1.068738 |

1,017 native ruleset iterations; 285,337 logical matches; 421,397 → 655,832 table rows. Separate diagnostic replay has identical canonical rows, values, visibility and per-rule match counts. Parent checkpoint is unchanged. No extraction occurs inside this packet.

## Complete queries at one real state inside that packet

The GPU uses explicit original table rows and the binary indexed table backend. This isolates backend matching; it neither invents Cell certificates nor applies original RHS actions. Before querying, execute the original `(seq terms terms-helpers)` once to expose real enabled cost updates. All 14 query binding sets agree exactly with native, totaling 28,373 bindings.

| Component, summed across 14 queries | Seconds |
|---|---:|
| Native full query, including query compilation/collection | 0.041693 |
| GPU cold query total, including all preparation | 1.617321 |
| Worker launch / static shader and buffer setup | 1.259700 |
| Original table scan / filtering / typed encoding | 0.157049 |
| Binary transfer + dispatch + results | 0.197958 |
| Host result binding reconstruction | 0.002401 |
| GPU device work, subset of roundtrip | 0.077608 |

Cold total also includes the small static plan compilation. Worker setup is paid once per rule here, not once per original saturation iteration. Native query timing includes query compilation, callback collection and canonicalization; it is not a pure Join timer. Common graph clone/action-sink preparation is excluded from both query paths and separately reported.

The cold GPU path includes worker startup and table preparation; compare its total with the complete native query, not the device timer alone. The device number includes index construction, joining, guards and active-match scanning; no proposal reduction. It is not isolated join-kernel time. Do not compare these one-state query totals with the packet wall time: the packet executes an evolving graph over 1,017 iterations.

## Queries, ordered by native query time

| Rule (first source line) | Bindings | Native ms | GPU cold ms | GPU transfer/dispatch/decode ms |
|---|---:|---:|---:|---:|
| `(rule ((PotentialExtractedExpr expr (TCPair term cost)) / (< cost oldcost))` | 158 | 12.093 | 121.413 | 12.762 |
| `(rule ((= lhs (Concat e1 e2))` | 3,172 | 5.187 | 133.401 | 13.632 |
| `(rule ((= lhs (Smaller (TCPair t1 cost1) (TCPair t2 cost2))) / (> cost1 cost2))` | 4,023 | 5.040 | 97.446 | 29.012 |
| `(rule ((= lhs (Smaller (TCPair t1 cost1) (TCPair t2 cost2))) / (< cost1 cost2))` | 1,563 | 4.699 | 154.083 | 41.909 |
| `(rule ((PotentialExtractedExpr expr (TCPair term cost)) / (< cost 0))` | 0 | 3.720 | 120.973 | 7.972 |
| `(rule ((= lhs (Get tup i))` | 3,604 | 2.942 | 83.152 | 11.591 |
| `(rule ((= lhs (Bop o e1 e2))` | 541 | 1.743 | 124.021 | 11.159 |
| `(rule ((= lhs (Smaller (TCPair t1 cost1) (TCPair t2 cost2))) / equal costs` | 247 | 1.688 | 120.852 | 9.396 |
| `(rule ((PotentialExtractedExpr expr termandcost))` | 12,356 | 1.477 | 63.175 | 8.037 |
| `(rule ((= lhs (Single e1))` | 1,294 | 1.434 | 112.785 | 9.778 |
| `(rule ((= lhs (Top o e1 e2 e3))` | 182 | 0.986 | 134.068 | 14.834 |
| `(rule ((= lhs (Const c ty ass)))` | 1,156 | 0.354 | 129.335 | 16.301 |
| `(rule ((= lhs (Uop o e1))` | 1 | 0.253 | 119.250 | 8.367 |
| `(rule ((= lhs (Arg ty ass)))` | 76 | 0.078 | 103.366 | 5.609 |

## Coverage and equality

| Measure | Result |
|---|---|
| Native/GPU complete query equality | 14/14, 28,373 complete bindings |
| False negative / false positive in those queries | 0 / 0 |
| Certified Cell shortcut coverage | Not measured; no Cell admission in this diagnostic |
| Expr / e-class / constructor coverage | Not measured; do not infer it from raw-table query equality |
| Whole GPU saturation / extraction quality | Not run; no end-to-end GPU speedup claimed |

Bulk upload 12,072,108 bytes; download 832,480 bytes. Small JSON controls, static program metadata and framing are excluded. No online solver is used. These totals include repeated tables across rules; the benchmark exposes rather than hides this cost.

The historical production pass profile separately splits RHS time from search, but it is not substituted for this fresh packet measurement. See README for the exact scope and reproduction.
