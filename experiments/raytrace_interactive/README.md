# Raytrace: interactive rule-family inspection

Measured 2026-10-10, one interactive diagnostic of the saved **real production
Pass 3 input**, plus one intact-schedule correctness control. The input was
produced after the preceding two production passes and their extractions;
it is not the original program independently run as "Pass 3". The current
`raytrace.rs` hash matches that saved capture.

This is a local modified native egglog replay. It does not rerun the full eggcc
frontend/extraction pipeline, bake Cells, or claim pristine performance. Existing
global atomic probe counters add overhead. Search+apply includes matching
management and RHS work. All times below use the **sum of per-rule timers**.

## Finding

The main measured family is `terms`, `terms-helpers`, and
`terms-helpers-helpers`: bottom-up candidate construction and guarded winner
selection. It takes **9.960 / 20.190 seconds (49.33%)** with only
335,420 / 3,093,625 logical matches (10.84%). This is a candidate for
**piecewise affine cost propagation with argmin and Term payload**, not an
already-proved affine RipSatCell family.

| Family | Matches | Search+apply seconds | Share |
| --- | ---: | ---: | ---: |
| Candidate construction and selection | 335,420 | 9.960 | 49.33% |
| Expr-size rules | 1,917,109 | 2.993 | 14.82% |
| Adjacent SubTuple concatenation | 12,132 | 0.221 | 1.09% |
| Affine SubTuple recursive unfolding | 8,113 | 0.00327 | 0.016% |

| Rule | Matches | Search+apply seconds |
| --- | ---: | ---: |
| Better PotentialExtractedExpr → update ExtractedExpr | 42,713 | 2.140 |
| Smaller, equal costs | 554 | 1.600 |
| Smaller, left cost smaller | 68,290 | 1.408 |
| Smaller, right cost smaller | 75,552 | 1.401 |
| Bop children → TermBop candidate | 16,730 | 0.942 |
| Concat children → TermConcat candidate | 6,162 | 0.776 |
| Negative candidate cost → panic | 0 | 0.445 |

Few or zero matches do not imply cheap search. The rows above are total rule
timers including unsuccessful search, **not per-action latency**. A logical
match is not a committed mutation, nor a count of repeated identical bindings.

The structural computation is:

```text
ExtractedExpr(e1) = (t1,c1), ExtractedExpr(e2) = (t2,c2)
    ↓
Bop(o,e1,e2) → candidate (TermBop(o,t1,t2), 1+c1+c2)
    ↓
candidate cost < old cost → update ExtractedExpr
    ↓
merge creates Smaller(old,new)
    ↓
compare TCPair costs, union Smaller with the winner
```

Candidate costs are affine in child costs. Winner selection is conditional;
Term constructors, equality, ties and update visibility remain part of the
effect. This does not justify replacing the entire family by an ordinary
numeric matrix or dropping its guards.

## Where to pause

The hottest packet is **349**, path `.../3/repeat0/4`: the first nested
`terms` saturation after `non-weakly-linear` and subsequent helpers.
It accounts for **7.417 seconds / 285,352 matches** and table rows increase from
421,397 to 655,833. That row difference is not a committed-mutation count.
Chronological placement alone does not identify which earlier optimization
caused the work.

Within this packet, the equal-cost Smaller rule has only **70 matches**, but
uses **1.159 seconds**. The complete packet performs 5,168,016 index-handle
probes and reports 13,969,228 candidate-row visits. These counters are sums
over the packet (including engine work), not per-rule attribution or a
single rejection funnel.

We interactively stopped before/after the first terms phase, continued the
original schedule, and inspected `Smaller` with native print commands. The
schedule journal locates the later hotspot above. The reproducible breakpoint
below lets the user stop directly at that later location.

## Reproduce and inspect

Use the cached production normalizer to expand production-only syntax such as
`unstable-fresh!` before loading it in the local native engine:

```sh
mkdir -p out/raytrace-inspect
../check_statewalk/target/release/flatten \
  out/eggcc-raytrace-pass-audit/raytrace/pass-3.egg \
  out/raytrace-inspect/pass-3-flat.egg
cargo run --offline --release --example rule_repl -- \
  out/raytrace-inspect/pass-3-flat.egg out/raytrace-inspect/live
```

At the prompt:

```text
:until /3/repeat0/4
:stats before-hot
:tables
:next
:stats after-hot
(print-size Smaller)
(print-function Smaller 2)
:finish
:stats complete
:quit
```

`:until terms-helpers` instead stops before the next terms saturation; the first
one is packet 50. `:next N` advances through planned packet positions, including
native repeat skips. `schedule.json` gives all native AST paths. `:stats NAME`
saves full rule text, match counts, timers, selected table sizes and the compact
packet journal. No raw match/mutation trace is generated.

The inspector exposes `Sequence` and `Repeat` boundaries, preserves Repeat's
`can_stop` early exit, and executes every `Saturate` atomically in the engine.
Manual native commands can change state: use print/check commands for an
unperturbed diagnostic. It is a replay REPL, not attachment to an existing
eggcc process or a GUI breakpoint facility.

## Validation and results

An intact command-stream control used the same source and binary:

```sh
target/release/examples/rule_repl \
  out/raytrace-inspect/pass-3-flat.egg out/raytrace-inspect/control --control
cargo test --offline --release --example rule_repl
```

All **382 per-rule match counts**, total table rows (**697,922**), and ten
selected table sizes agree. Four small schedule cases also compare native and
split execution, including nested repeats and early termination. This is not
a full graph-isomorphism or extraction-quality comparison. No timing average
is taken from the control.

[results.json](results.json) contains the findings, full hot-rule definitions,
control checks and hashes. [rules.csv](rules.csv) lists every observed rule;
the large initialization fact body is represented by its hash and source pointer.
Raw summaries are in `out/raytrace-interactive-20261010/live`; they are a few MB,
not a full trace. Recreate the compact result with `summarize.py LIVE CONTROL
--output DIRECTORY` using this saved capture layout.
