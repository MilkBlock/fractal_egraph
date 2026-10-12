# Canonical marker transitions

These are actual generated `.egg` programs running on the local native egglog
runtime. The compiler has no special case for Plus, commutativity, or chain length.
Native validation compares complete constructor rows and equality partitions
against the original source. This is not a pristine performance benchmark.

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/canonical_markers/run.py \
  --output out/canonical-markers-new --skip-build
```

| Source | Original / restored constructors | Original table rows | All spine-side table rows before export |
|---|---:|---:|---:|
| Three-input AC | 15 / 15 | 16 | 12 |
| Seven-input spine + external graft | 18 / 18 | 19 | 25 |
| Two instances sharing a tail | 10 / 10 | 11 | 14 |
| Sibling-root guard | 6 / 6 | 6 | 9 |
| eggcc If subset | 60 / 60 | 60 | 87 |

Totals include raw ingress constructors, canonical constructors, active and
subsumed markers, and named bindings. The extra named `root` explains the one-row
difference between some original constructor and table counts. No Read/Wanted
relations exist. Most of these tiny cases still have more physical rows than the
original: the result proves correct gating and reuse, not universal compression.
Library byte sizes and all per-table counts are in `report.json`.

## What the graft actually does

The source's first rule recognizes a seven-input right chain and derives Ready.
Its second rule consumes Ready plus Extend and derives the completed result.
This is not an AC family shortcut. Bake found both finite Cells and their causal
connection from the actual source execution.

Gurobi selects the canonical paths and active-marker layouts. On this fixture it
selects the root as attachment point. The emitted main transition, with variable
names shortened, is:

```egg
(rule ((Spine-Summary-0 root) (= graft (Spine-Extend root extra)))
      ((Spine-Cano-Extend root extra)
       (union graft (Spine-Cano-Extend root extra))
       (Spine-Summary-1 graft)
       (subsume (Spine-Summary-0 root)))
      :ruleset Spine-Run)
```

There are no old Plus lookups or Plus constructions in this transition. The old
root is reused directly. The final state has zero active old markers and one
active new marker. Cached export restores all 18 original constructors. Separate
inherited consumers keep already baked later continuations available after the
old marker is retired; they do not revive that old marker.

## Guards that are checked

- An unmarked `Spine-Cano-*` node cannot bootstrap a Cell.
- Inference cannot write raw `Spine-*` constructors and feed its own output back
  into normalization. Every canonical lookup has an active marker guard.
- A larger canonical seed can retire covered markers at its root and in its
  canonical subtrees, rather than only its recorded producer parent.
- A second instance sharing the tail stays active when the first instance grows.
- Unioning A(a) with A(b) does not authorize B(b) merely because a Cell contains
  A(a) and B(a). Erased bindings retain all seed correlations and equality guards.
- The root-union regression forces a compact marker with erased fields, so it
  cannot pass merely by retaining the original full binding tuple.

`marker_layouts` lists attachment points and stored fields. `gate_audit` lists the
actual marker/canonical/external lookups after lowering. Gurobi's objective is an
explicit structural estimate; inherited code and dead-decoder elimination are
reported separately. Equivalent-product factorization reduces the ILP size
without dropping candidates, imposing a budget or weakening the optimum.

The frozen three-input library's negative seven-input AC test remains in
`../reuse_boundary/`. It does not acquire missing states merely by adding a tag.
