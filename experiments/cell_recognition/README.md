# Event recognition vs whole-fragment discovery, with node attribution

Compares six variants on the same source and schedule:

| variant | what it does |
|---|---|
| `pristine` | the recorded baseline binary runs the program |
| `native` | this kernel runs the program, no library |
| `fragment_pack` | this kernel plus whole-fragment discovery, then packs every match |
| `ripsat_off` | the adaptive runner in `native` mode: source rules only, audit trace on |
| `ripsat_on` | the adaptive runner in `frozen` mode: baked closures replace the source RHSs, audit trace on |
| `recognition` | this kernel plus event recognition, then installs recognised cells |

The point of the change is the last row's `fragment_queries`: the default
known-instance path must issue **none**. The harness fails when it is not zero.

## Node attribution

`node_origin::attribute` walks every **live** constructor row and asks the runtime
which rule application produced it (`EGraph::row_producer_row`). Counting live
rows, not write events, is what makes merge deduplication implicit: a row a union
merged away is not live, so it cannot be counted twice. Rows are bucketed by rule
name, and the adaptive runtime names every generated rule
`adaptive_{baked|source}_{id}`, so:

- `seminaive` — produced by an original source rule;
- `ripsat` — produced by a RipSat closure rule;
- `unattributed` — no retained producer, split into `no_producer` (the runtime kept
  none for this row) and `unknown_match` (a producer exists but its match is not in
  the retained rule map).

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

| variant | wall | peak RSS | total nodes | seminaive | ripsat | unattributed |
|---|---:|---:|---:|---:|---:|---:|
| pristine | 0.009 s | 7.2 MB | — | — | — | — |
| native | 0.008 s | 7.7 MB | — | — | — | — |
| fragment_pack | 0.017 s | 10.8 MB | — | — | — | — |
| ripsat_off | 0.016 s | 8.2 MB | 208 | **0** | **0** | 208 |
| ripsat_on | 0.020 s | 11.6 MB | 347 | **0** | **0** | 347 |
| recognition | 0.221 s | 103.9 MB | — | — | — | — |

**Timings are not a speed comparison**: one pass at millisecond scale, and
`native` (0.008 s) and `native` at 0.257 s in the previous pass are the same
command. The scenario row above is the stable part: `ripsat_on` holds 347 live
constructor nodes against `ripsat_off`'s 208, i.e. the baked closures produce 139
more nodes in this run.

**The seminaive/ripsat split is not yet populated, and this is the current
blocker.** With the audit trace on, the rule map is populated (`rules_known` 129
for `ripsat_off`, 211 for `ripsat_on`), but every row reports `no_producer`: the
runtime retained no per-row producer for these tables, so `unknown_match` is 0 and
`unattributed` equals `total`. The attribution code and the runtime accessor are in
place and exercised; what is missing is provenance retention on this runner's write
path. The diagnostic fields exist precisely so that this is distinguishable from
"the split is 0".

Two further limits: `pristine`, `native`, `fragment_pack` and `recognition` do not
attach a trace, so they report `node_origin: null` rather than a fabricated zero;
and per-stage timings and unfinished-instance retained memory are still not
measured (`pending_subscriptions` is the only proxy).

## Verified elsewhere

`recognize-cells` installs through `compact_cell_facts`, which re-validates every
row against committed storage, and `tests/reuse_cell_recognition.rs` checks that
installation leaves the graph's rows unchanged. A repeated, equal-work benchmark on
the full math workload remains to be done.
