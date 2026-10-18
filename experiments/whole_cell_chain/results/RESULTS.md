# Whole RipSatCell execution results

A → B → C executes complete closed states. Bake, the independent native reference and the post-run audit are separate from the hot runtime. The original rules remain installed in that runtime.

| Stage | Cell state | Cell enodes | Inherited enodes | Emitted enodes | Native smooth matches | Runtime smooth matches | Macro worklist matches | Full graph nodes/classes | Extraction cost |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A | 0 | 2 | 0 | 2 | 2 | 0 | 1 | 6/5 | 5 |
| B | 1 | 10 | 2 | 8 | 12 | 0 | 1 | 14/8 | 8 |
| C | 2 | 18 | 10 | 8 | 12 | 0 | 1 | 22/11 | 11 |

**Every stage passes exact full original-graph comparison**, including literals and e-class equality; these are not only count comparisons. Original rule watermarks are unchanged. Each target frame is returned once by the native commit receipt. Macro worklist dispatch and constructor/hashcons/union work are not claimed to disappear.

The retained parent rows are transported through the target equality map before computing the delta. The compiler can therefore emit unions of old slots and rely on native rebuild to merge old keys; it is not plain set subtraction on concrete node IDs.

A separate audit runs the original matcher on a clone of each macro result. It finds matches but changes no facts. This proves the closure was already produced; those audit matches are never used by the runtime.

Final extracted term (same cost as the independent native run):

```egg
(Mul (Var 307) (Add (Var 409) (Add (Var 101) (Var 203))))
```

The plan contains only coarse graft contexts. Target bodies and deltas were obtained by actual symbolic native saturation, then verified by delta replay from parent snapshots. Tests additionally reuse the same library with unseen leaf identities, exercise non-root grafts, merge old parent slots, preserve cyclic bodies, and reject extra-but-saturated graphs.

This is a CPU/native semantic acceptance result, not a throughput benchmark, GPU speedup or virtual storage result. The current contract is positive constructor/union rules, inert input forests, exact alias conditions and full saturation boundaries. General eggcc effects, finite schedules and automatic runtime graft discovery are not covered.
