# Event recognition vs whole-fragment discovery, with node accounting

Compares six variants on the same source and schedule:

| variant | what it does |
|---|---|
| `pristine` | the recorded baseline binary runs the program |
| `native` | this kernel runs the program, no library |
| `fragment_pack` | this kernel plus whole-fragment discovery, then packs every match |
| `ripsat_off` | the adaptive runner in `native` mode: source rules only, audit trace on |
| `ripsat_on` | the adaptive runner in `frozen` mode: baked closures replace the source RHSs, audit trace on |
| `recognition` | this kernel plus event recognition, then installs recognised cells |

The point of the recognition change is the last row's `fragment_queries`: the
default known-instance path must issue **none**, and the harness fails when it does
not.

## Node accounting

A per-row question - "which rule produced this node" - cannot be answered after a
run that unions. The runtime drops a row's retained producer when a rebuild
rewrites that row (`PendingState::invalidate_origin`: *"Rebuild still invalidates
changed rows; this does not assert lineage through canonicalization"*), and a
commutativity saturation rewrites nearly every row. Asking live rows therefore
returns "no producer" almost everywhere, which measures nothing. An earlier
version of this harness did exactly that and reported every node as
`unattributed`; that column was a failure indicator, not a result, and it is gone.

`node_origin` now accounts for the **committed write stream** and reconciles it
against the live graph:

* `inputs` - constructor nodes before the first traced round;
* `inserted` / `deduplicated` / `updated` / `unsupported` - committed writes to
  constructor tables, bucketed by the producing rule;
* `live_total` - constructor nodes measured in the final graph;
* `removed_or_merged = inputs + inserted - live_total` - measured, not assumed.

Bucket rule, from the naming contract in `reuse_adaptive::rule_program`:
`adaptive_baked_{id}` is a rule whose RHS a learned macro replaced (a RipSat
closure rule), `adaptive_source_{id}` and any other name are original source
rules, and a write whose match is not in the retained rule map is its own bucket.
The `adaptive_` prefix alone does **not** separate them: in this runner every rule
carries it.

## Reproduce

```sh
EGG_LAYOUT_GRAPHS=0 egg_layout analyze --source experiments/math_mini3/source.egg \
  --rounds 1 --output out/mini-seed
egg_layout reuse-bake out/mini-lib.json out/mini-seed
cargo build --release --offline --examples
python3 experiments/cell_recognition/bench.py \
  --source experiments/math_mini3/source.egg --library out/mini-lib.json \
  --output out/cell-bench-mini --rounds 1
```

## Measured (math mini3, one round, one pass)

| variant | wall | peak RSS | live nodes | seminaive (gross) | ripsat (gross) | removed/merged |
|---|---:|---:|---:|---:|---:|---:|
| `pristine` | 0.026 s | 6.9 MB | - | - | - | - |
| `native` | 0.232 s | 7.2 MB | - | - | - | - |
| `fragment_pack` | 0.064 s | 10.7 MB | - | - | - | - |
| `ripsat_off` | 0.536 s | 8.3 MB | 208 | 173 | 0 | 0 |
| `ripsat_on` | 0.064 s | 11.7 MB | 347 | 33 | 484 | 205 |
| `recognition` | 0.240 s | 97.8 MB | - | - | - | - |

`inserted` counts every committed `Inserted` write before dedup, as requested;
`deduplicated` and `updated` are listed separately in `results.json` so the
absorption is visible rather than netted out:

| variant | inputs | inserted seminaive | inserted ripsat | deduplicated seminaive | deduplicated ripsat | updated | live | removed/merged |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `ripsat_off` | 35 | 173 | 0 | 37 | 0 | 1 | 208 | 0 |
| `ripsat_on` | 35 | 33 | 484 | 0 | 760 | 1 | 347 | 205 |

What this says, for this workload and run:

* With the closures off, every produced node is a source-rule node:
  35 inputs + 173 inserted = 208 live, residual
  0; merge absorbed
  37 proposals without removing any node.
* With the closures on, the RipSat path accounts for 484 of the
  517 committed insertions and the source rules for
  33; merging then removed
  205 nodes, leaving 347.

Timings remain one pass at millisecond scale and are not a speed comparison.
Per-stage timings and unfinished-instance retained memory are still not measured;
`pending_subscriptions` is the only proxy.
