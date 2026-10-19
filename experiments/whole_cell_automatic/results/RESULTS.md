# Automatic whole-Cell acceptance

One actual native run; CPU event dispatcher, not a GPU or speedup benchmark.

| Native coarse step | Whole graph e-nodes / e-classes | Native smooth matches | Macro calls (cumulative) | Fallbacks (cumulative) |
|---|---:|---:|---:|---:|
| 1 | 14 / 12 | 4 | 4 | 0 |
| 2 | 30 / 18 | 24 | 8 | 0 |
| 3 | 46 / 24 | 24 | 12 | 0 |

Native smooth matches: **52**. Automatic-path fallback smooth matches: **0**.
Each accepted macro also checks that original smooth query watermarks did not advance.
Whole original graphs are exactly equivalent at all three boundaries (including the two Start rows).

The two inputs share the Bake. Add commutativity exposes both x/y slot orientations,
so positive certificates retain multiple frames; these are reported rather than called one transition.

Final counters:
```json
{
  "committed_rows": 46,
  "coverage_rows_checked": 60,
  "fallback_smooth_matches": 0,
  "graft_candidate_rows": 26,
  "invalidated_instances": 0,
  "macro_transitions": 12,
  "native_fallbacks": 0,
  "rejected_specializations": 0,
  "union_events": 22
}
```

Runtime transition edges (instance IDs retain overlapping source views):
```text
A: None -> 0 (state None -> 0)
A: None -> 1 (state None -> 0)
A: None -> 2 (state None -> 0)
A: None -> 3 (state None -> 0)
B: 0 -> 4 (state 0 -> 1)
B: 1 -> 5 (state 0 -> 1)
B: 2 -> 6 (state 0 -> 1)
B: 3 -> 7 (state 0 -> 1)
C: 4 -> 8 (state 1 -> 2)
C: 5 -> 9 (state 1 -> 2)
C: 6 -> 10 (state 1 -> 2)
C: 7 -> 11 (state 1 -> 2)
```

Reproduce with the command in ../README.md. No wall-clock speedup is asserted.
