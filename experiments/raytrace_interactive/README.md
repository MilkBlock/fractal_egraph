# Raytrace: interactive rule-family inspection

Follow-up: Pass 1 and Pass 2 have now also been replayed once with the same
binary. The initial finding below was specifically about Pass 3; it must not
be applied unchanged to the earlier passes. See the cross-pass section below.

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

## Follow-up: the first two passes differ

New intact-schedule replays of the saved production Pass 1 and Pass 2 inputs
use the same local instrumented `rule_repl` binary as the Pass 3 inspection.
Pass 3 below reuses that earlier diagnostic; no averages are taken. This is
not a new end-to-end production run or a new extraction-quality measurement.

| Pass | Logical matches | All per-rule search+apply | Terms-family matches | Terms-family time | Share |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 924,275 | 4.077 s | 34,524 | 0.366 s | 8.99% |
| 2 | 1,767,619 | 10.804 s | 142,470 | 3.404 s | 31.50% |
| 3 | 3,093,625 | 20.190 s | 335,420 | 9.960 s | 49.33% |

Pass 1's top rule is Concat type propagation (6,578 matches / 0.365 s).
Other visible costs are Concat size propagation (150,519 / 0.305 s), a loop
invariance rule joining BodyContainsExpr, DoWhile, Arg and Get
(319 / 0.216 s), and adjacent SubTuple merging (10,735 / 0.170 s).
The rest of always-run excluding Expr-size is 41.54% of Pass 1 per-rule time;
type-analysis is 24.44%, Expr-size is 17.84%.

Pass 2 has mixed hotspots: better-candidate selection (21,748 / 0.682 s),
Concat Term candidate construction (32,190 / 0.634 s), Concat type propagation
(40,323 / 0.569 s), and equal-cost Smaller resolution (7,540 / 0.433 s).

### What "piecewise affine" and "Join size" actually mean here

For one fixed binding, a Bop candidate has cost `1+c1+c2`. Selecting the
smaller of that cost and the old cost is piecewise affine (within defined
i64 arithmetic). This description applies only to the scalar update, not to
the complete Term construction, equality changes, guard search or Cell state.

The hot rules have modest logical LHS table-atom counts:

| Rule | Logical table atoms | Additional condition |
| --- | ---: | --- |
| Smaller resolution | Smaller + two TCPair = 3 | cost comparison/equality |
| Better candidate | PotentialExtractedExpr + ExtractedExpr + two TCPair = 4 | new cost < old cost |
| Bop/Concat candidate | constructor + two ExtractedExpr + two TCPair = 5 | none beyond bindings |
| Top candidate | constructor + three ExtractedExpr + three TCPair = 7 | none beyond bindings |

These are neither physical stage counts nor visited-row counts. A TCPair
result is an e-class value; finding its Term and cost is another relational
lookup, not reading two ordinary fields of a Rust struct. The final Pass 3
tables have 63,051 TCPair, 72,198 Smaller and 69,649 PotentialExtractedExpr
rows. Many repeated searches over those relations can be costly even when
only a few complete matches survive. This report does not separately measure
per-rule index-handle time, probe time or intermediate candidate counts, and
therefore does not diagnose a Cartesian-product explosion or a bad key choice.

[passes.json](passes.json) gives per-pass groups and top rules;
[all-pass-rules.csv](all-pass-rules.csv) contains all observed per-rule counts.
The comparison script validates sums and records input/runtime hashes.

```sh
mkdir -p out/raytrace-interactive-20261010/pass1 out/raytrace-interactive-20261010/pass2
../check_statewalk/target/release/flatten out/eggcc-raytrace-pass-audit/raytrace/pass-1.egg out/raytrace-interactive-20261010/pass1/source.egg
target/release/examples/rule_repl out/raytrace-interactive-20261010/pass1/source.egg out/raytrace-interactive-20261010/pass1 --control
../check_statewalk/target/release/flatten out/eggcc-raytrace-pass-audit/raytrace/pass-2.egg out/raytrace-interactive-20261010/pass2/source.egg
target/release/examples/rule_repl out/raytrace-interactive-20261010/pass2/source.egg out/raytrace-interactive-20261010/pass2 --control
python3 experiments/raytrace_interactive/compare_passes.py
```
