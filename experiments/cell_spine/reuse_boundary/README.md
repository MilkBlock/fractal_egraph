# Frozen-library coverage boundary

This probe bakes the existing three-input `assoc.egg` fixture once, freezes its
library, then validates six other inputs in native egglog. It compares complete
constructor states and equality partitions. Counts alone are not a certificate.

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/reuse_boundary/probe.py \
  --output out/reuse-boundary-new --skip-build
```

## Current active-marker backend

| Input | Expected original nodes | Restored nodes | All stored rows before export | Result |
|---|---:|---:|---:|---|
| 3, left | 15 | 15 | 11 | equivalent |
| 3, right | 15 | 7 | 9 | rejected |
| 4, left | 54 | 26 | 17 | rejected |
| 4, right | 54 | 10 | 13 | rejected |
| 7, left | 1,939 | 59 | 35 | rejected |
| 7, right | 1,939 | 19 | 25 | rejected |

**The smaller rejected graphs are not compressed versions of the expected graph.**
They are incomplete. Current inference uses baked canonical transitions, and no
longer publishes every cached internal row into a global query table. The old
three-input library lacks some transitions required by these new shapes/depths.
This coverage loss is explicit, not hidden behind a storage ratio. Re-baking a
right-associated three-input source succeeds with its own observed paths; that
is different from promising arbitrary-depth reuse of the left-associated library.

Generated failed `*-runtime.egg` and `*-restore.egg` files carry a REJECTED comment.
Production Cell compilation still performs native whole-source validation before
publishing executable output. No original inference rules run during compressed
inference or cached export. Finite `(run add-ac 7)` is not silently replaced by
saturation.

## Preserved comparisons

- `before-frontier.json`: pre-interface negative baseline from ab9ed1d.
- `flat-interfaces.json`: 7fa61af's global Read/Wanted implementation. All six
  graphs were equivalent, but seven inputs stored 16,246 rows, including 1,932
  Read rows, 1,925 Wanted rows and 12,138 Summaries. That implementation has been
  removed: its global reads bypassed marker retirement.
- `report.json`: current active-marker result above. Three-left needs 11 rows
  rather than 46 with flat interfaces. Both restore 15 constructor rows. This
  says nothing about RSS; rows have different widths.

The current positive tests, including a seven-input spine followed by one baked
external graft, are in `../canonical_markers/`. That graft example is not the
seven-input AC closure and must not be substituted for this negative test.
