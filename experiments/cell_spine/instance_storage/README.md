# Current-input storage pricing

This is the native compressed egglog runtime, not pristine upstream and not a
new storage engine. Original inference and closure export do not run while
pricing the compressed program.

## Reproduce

After building the usual compiler/native tools and installing Gurobi:

```sh
RAYON_NUM_THREADS=1 out/cell-spine-venv/bin/python tools/bench/storage_pricing.py \
  --source experiments/cell_spine/fixtures/input_spine_left.egg \
  --output out/storage-pricing-comparison

out/cell-spine-venv/bin/python tools/bench/ci_cell_spine.py \
  --output out/storage-pricing-smoke --cases left --copies 16 \
  --storage-cost instances
```

The first command Bakes both objectives against the same captured catalog.
The normal compiler exposes `--storage-cost instances`; `structural` remains the
default because the exact measured value function can make large Bakes costly.
No model is silently truncated to fit the limited Gurobi license.

## Observed on 2026-10-13

For the three-term input, both objectives select the same program:

| Stored table | Rows | Fields per retained row |
|---|---:|---:|
| `Leaf!` | 3 | 4 |
| `Add!!` | 2 | 5 |
| `Summary-1!` | 1 | 4 |
| global `root` | 1 | 2 |
| Total | 7 | 28 Value slots |

Fields include output, timestamp and subsume flag where present. Shared rows
are counted once. There is **no new compression improvement on this input**.
Mandatory-input lower bounds and alpha-equivalent program reuse reduce native
pricing to five executions (~0.28 s of storage-pricing work in this local run).
This is a development observation, not a timing significance claim.

The separate 16-copy native smoke test produced:

| Execution mode | Pre-export rows | Expanded e-nodes | Expanded e-classes |
|---|---:|---:|---:|
| Native original program, current kernel | 256 | 240 | 112 |
| Compact library with raw ingress | 192 | 240 | 112 |
| Selected input spine | 112 | 240 | 112 |

These row reductions already existed before this cost-model change. The new
work makes retained storage visible to selection. Unit tests additionally
demonstrate a choice reversal when a narrower marker produces many more stored
rows; this synthetic selection test is not a measured workload speedup.

## Boundaries

- The priced quantity is the larger of input/end retained Value-slot counts,
  not allocated bytes, RSS or a within-sweep peak.
- Retired markers remain charged. Native union/hashcons perform actual key
  coalescing; final identities are not projected backward onto initial storage.
- Materialized index shapes are reported. Index capacity, hashcons capacity,
  union-find, stale backing rows and transient buffers remain unpriced.
- The existing minimum-initial-recipe policy is conditional on each ABI.
  Alternative initializers that spend more initially to save later storage are
  not exhaustively optimized. The exact priced initializer is emitted unchanged.
- Long/multi-family layouts still make exact native pricing expensive. The
  default full CI retains structural pricing and runs a separate small instance
  ablation; no large-catalog performance improvement is claimed.

Full native closure validation remains a publication gate. The compiler also
compares every retained table's row count against its selected priced program.
