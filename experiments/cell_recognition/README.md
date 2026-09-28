# Event recognition vs whole-fragment discovery

Compares four variants on the same source and schedule:

| variant | what it does |
|---|---|
| `pristine` | the recorded baseline binary runs the program |
| `native` | this kernel runs the program, no library |
| `fragment_pack` | this kernel plus whole-fragment discovery, then packs every match |
| `recognition` | this kernel plus event recognition, then installs recognised cells |

The point of the change is the last row's `fragment_queries`: the default
known-instance path must issue **none**. The harness fails when it is not zero.

## Reproduce

```sh
# one library per workload; both formats are unchanged by this work
EGG_LAYOUT_GRAPHS=0 egg_layout analyze --source experiments/math_mini3/source.egg \
  --rounds 1 --output out/mini-seed
egg_layout reuse-bake out/mini-lib.json out/mini-seed

cargo build --release --offline --example post_pack_bench
python3 experiments/cell_recognition/bench.py \
  --source experiments/math_mini3/source.egg --library out/mini-lib.json \
  --output out/cell-bench-mini --rounds 1
```

`bench.py` honours `CARGO_TARGET_DIR`, writes `<output>/results.json`, and exits
non-zero if the recognition variant reports a fragment query.

## Measured (math mini3, one round, one pass)

| variant | wall | peak RSS |
|---|---:|---:|
| pristine | 0.009 s | 7.1 MB |
| native | 0.449 s | 7.5 MB |
| fragment_pack | 0.032 s | 11.9 MB |
| recognition | 0.233 s | 99.0 MB |

**These timings are not a speed comparison and must not be read as one.** On this
workload the measurements are unstable: `native` (0.449 s) and `fragment_pack`
(0.032 s) run the same program, and the difference is run-to-run variance at
millisecond scale, not a library effect. The recognition row additionally loads a
library, compiles 8 plans and keeps 129 outstanding row subscriptions, which is
why its RSS is an order of magnitude larger.

What the run does establish, stably:

| quantity | value |
|---|---:|
| fragment queries on the default path | **0** |
| plans / sources | 8 / 19 |
| sources with a slot boundary | 19 |
| matched committed applications | 19 |
| located / complete / packed | 14 / 42 / 11 |
| unsupported sources | 0 |

Remaining failures are evidence gaps, not structural ones: 11 instances report
`no resolvable row left` and 3 name a specific row whose key is not a committed
fact (`row 3 (Add) key [43, 42] ...`) because this short run never executed that
step.

## Scope and limits

- One screening pass per variant, one round, one machine; no repetition, no
  median, no warmup. The earlier experiments in this repository (`math_pack_rss`,
  `compact_cells`, `memory_fix`) established that peak-RSS comparisons on this
  system need repetitions and a recorded pristine hash; this harness does neither
  yet.
- Timing, recognition time and install time are not reported separately: the
  recognition variant prints one total, and the kernel does not yet expose install
  duration. Per-stage timings are a missing measurement, not a passing one.
- Unfinished instances' retained memory is not measured. `pending_subscriptions`
  (129) is the only proxy reported.
- The four variants do not all do equal work: `fragment_pack` and `recognition`
  load a library; `native` and `pristine` do not. That is the subject of the
  comparison, but it also means wall time is not an equal-work speed ratio.
- The recognition path's logical result was verified elsewhere: `recognize-cells`
  installs through `compact_cell_facts`, which re-validates every row against
  committed storage, and `tests/reuse_cell_recognition.rs` checks that the graph
  is unchanged by installation.

A repeated, equal-work benchmark on the full math workload — with per-stage
timings, a hash-verified pristine binary and medians — remains to be done.
