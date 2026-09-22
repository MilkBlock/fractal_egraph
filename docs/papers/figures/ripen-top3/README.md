# Native top-3 ripen graphs

These files are the three highest-coverage saturated bodies from the default
four-round `math-microbenchmark.egg` capture used by paper v0.2.4:

| Rank | Catalog state | Covered CS layers | Native constructors |
| ---: | ---: | ---: | --- |
| 1 | C0 | 6 | `RipenInput`, `Add` |
| 2 | C1 | 1 | `RipenInput`, `Add` |
| 3 | C4 | 1 | `RipenInput`, `Mul` |

Each `.dot` and `.svg` was written from the actual isolated ripen `EGraph`
through `egraph_serialize`. The `.pdf` is Graphviz's direct rendering of the
same DOT file for inclusion in LaTeX. These are actual egglog runtime graphs,
not a standalone visualization prototype and not a `Node`/`Row` encoding of
the catalog state. For older runs, the exporter reruns the recorded `entry.egg`
in a temporary directory when the native files are absent.

The selected entry programs came from:

```text
out/default-ripen-math4/saturated-rule-composition/cells/CSUnit-000000/entry.egg
out/default-ripen-math4/saturated-rule-composition/cells/CSUnit-000002/entry.egg
out/default-ripen-math4/saturated-rule-composition/cells/Use-000005/entry.egg
```

They were rerun with `--max-rounds 8`; all three reported `Saturated` and
passed their checks. The coverage ranking comes from
`docs/papers/figures/ripen-top100.json`.
