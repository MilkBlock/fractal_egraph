# Independent reproduction of `eggcc_cell_probe`

I re-ran codex's probe end-to-end on a fresh analysis directory with the current
tree (`cb76639`), release build, and compared against the checked-in
`results.json`.

```sh
cargo build --release --offline --example eggcc_cell_probe
python3 experiments/multiple_datatypes/run.py \
  --eggcc /Users/mineralsteins/Repos/egg_related/check_statewalk \
  --analyzer target/release/egg_layout --output /tmp/eggcc-probe-verify
target/release/examples/eggcc_cell_probe /tmp/eggcc-probe-verify/live /tmp/cell-short 50
target/release/examples/eggcc_cell_probe /tmp/eggcc-probe-verify/live /tmp/cell-long 500
python3 experiments/eggcc_cell_probe/summarize.py \
  /tmp/eggcc-probe-verify/live /tmp/cell-short /tmp/cell-long /tmp/cell-summary
```

## Result: every aggregate metric matches

`source_sha256` is `7747960d4e1aefb1…` in both runs. All ten `short_observation`
and all ten `long_observation` metrics agree exactly:

| metric | 50 ms | 500 ms |
|---|---:|---:|
| fixed point / unfinished | 140 / 4 | 140 / 4 |
| completed initial groups | 68 | 68 |
| distinct final states | 44 | 44 |
| final groups with multiple occurrences | 15 | 15 |
| occurrences in shared groups | 111 | 111 |
| final groups merging different initial states | 11 | 11 |
| groups with distinct recorded boundary bindings | 12 | 12 |
| net row growth above the 3-row empty control | 126 | 126 |

Largest shared group also matches: group 20, 26 occurrences, 14 binding
signatures, initial 3–4 rows, final 11 rows.

Warm vs fresh continuation (`grafts`), reproduced:

| binding (abridged) | warm m/r | fresh m/r | saved |
|---|---:|---:|---:|
| `Arg(IntT,StateT)` | 187 / 100 | 199 / 107 | **12 / 7** |
| `Arg(StateT,IntT)` | 187 / 100 | 199 / 107 | **12 / 7** |
| `Call "fact" (Sub…)` | 305 / 167 | 317 / 174 | **12 / 7** |
| `Call "fact" (Get…)` | 225 / 118 | 237 / 125 | **12 / 7** |
| `If (Eq …)` | 6205 / 3128 | 6212 / 3130 | 7 / 2 (still unfinished) |

Only the wall-clock fields differ (e.g. 0.0197 s vs 0.0243 s at 50 ms), which is
expected and is not quoted anywhere as a speedup.

## What the reproduction adds

The probe is deterministic and its checked-in numbers are honest. Two things are
worth stating explicitly:

1. **The saved-work size is ~6%, not a headline.** 12 fewer matches of 199 is
   6.0%; 7 fewer receipts of 107 is 6.5%. That is per continuation of one small
   cell, not a whole-run saving, and it is the *best* case in the sample.
2. **The observation limit is not the binding constraint here.** 50 ms and
   500 ms give identical statuses (140/4). The four unfinished cases grow with
   *passes*, not with time — consistent with the `swap-if` ablation
   (18/18/18/24 rows once it is removed).

## What is still not measured

The probe deliberately bypasses the production export gate. In the same fresh
run the production path still reports, for all 2,060 entries:

| rejection | count |
|---|---:|
| `set commits interleave match order` | 1,194 |
| `automatic ripen cannot export its trigger state` (container field sort) | 620 |
| `set witness needs a scalar literal or an existing symbolic binding` | 110 |
| others | 136 |

So the probe establishes that the *signal* exists; the production path still
does not admit it. The `last_commit` scope issue I flagged is unchanged in
`c243a4c` (`let mut last_commit = 0` is still initialised outside the member
loop and compared across records, while only 7 of 2,777 history records carry
two `set`s).
