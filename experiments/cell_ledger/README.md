# Automatic Cell ledger validation

The bench skill now generates Markdown, CSV and JSON work ledgers. This saved
example is generated exclusively by `bench.py cell-ledger`; no experimental
one-off statistics script is needed.

The actual full raytrace Pass 3 runs use worker layout 17 with diagnostic
counters. Both final graphs match the saved same-submit CPU control; the
pre-existing fused-native discrepancy remains. These runs are **not timing
measurements**, and no previous uninstrumented speedup is reattributed to them.

The ledger lists all 5 loaded Analyze Cells, 6 normalized shared projections,
31 source-compiled CPU programs, the joint proof group and 13 projection
consumers. Cell 0 has no compiled projection. Cells 1–4 justify the same shared
projections and therefore link to one work account rather than each claiming
its full savings.

Only projection 2 is consumed: 76 / 6,756 segment requests succeed (1.125%),
and 60 / 92 ready-port evaluations are nonempty (65.22%). It performs 11,367
fresh certification probes and reuses 18 witness rows. Projections 0/4/5 each
perform 416 fresh probes but receive no segment requests and publish no ready
ports. All counters, including zeros, remain in the files.

The unified per-query live-row counters, including general certificate
execution and port residual matching, now show **168 candidate visits saved**,
84 on each of the two interval-analysis consumers. This is different from the
older 440 ordinary-index-probe difference; candidate visits and probes are
not interchangeable. The 168 visits are recorded once for the connected
plan–consumer sharing group, not four times for its source Cells.

273,075 watch bucket entries were inspected but none passed the current
version/table/width filter. They remain shared unassigned work, rather than
being charged to the new owner of a recycled instance slot. No per-Cell seconds
or independently causal credit is fabricated from those counts.

Reproduction (read/derive only):

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py cell-ledger \
  --input out/cell-ledger-reuse --baseline out/cell-ledger-plan \
  --output experiments/cell_ledger/ledger
```

Normal future `run --all-rules --analysis DIR` generates the table automatically
and reuses or collects only the missing independent diagnostics. No automatic
N-way per-Cell timing sweep is added. `--no-cell-metrics` disables extra runs
while preserving available rows and explicit unmeasured fields.

Validation: 15 full-GPU and 5 projection regressions pass with diagnostics;
47 Python workflow/ledger tests pass. Two full Pass-3 graph checks pass, and
repeating the profile command reuses the saved result without executing the
engine or creating the requested new directory. Cached companion selection and
nonrecursive collection are also covered by workflow tests.
