# Source-level spine markers on the real raytrace Pass 3

This is an actual **native CPU egglog** run of a generated `.egg`, not the GPU
receipt bridge, not a standalone graph simulation, and not whole-Cell sat-skip.
The input is the existing complete `pass-3-flat.egg` snapshot and its main
schedule. The frozen Analyze library is `experiments/analyze_library_credit/library`.

## What is connected

`tools.cell_spine.pass3_markers` uses the native parser, typed constructor
signatures and the existing Analyze-state matcher to find shared, fully keyed
two-atom spines. An exact MIP chooses non-overlapping occurrences. This is a
structural schema cover, **not** a claim of native-time global optimization.
Only scheduled rules count; combined rulesets are expanded. Constructor tables
subject to delete/subsume are excluded. Mutable reads, primitive guards and the
original RHS remain outside the view. Input facts and all original public tables
remain explicit; no original closure is hidden or replaced with a whitelist.

The output has:

- Unit-valued marker functions. In this fork, `relation` desugars to a fresh
  constructor sort; using an explicit Unit function avoids that identity cost.
- Exact completion rules before each original Run, covering pre-existing facts
  and union-enabled bindings. This maintenance costs real search and writes.
- Optional publication from the known LHS/RHS observations of an original
  application. Native let-bound DAGs stay shared; equivalent symbolic rows and
  bindings are deduplicated. Later atoms use complete keys rather than scanning
  the product of all observed rows. Mutable expressions are not reevaluated to
  invent a witness after a set.
- Original finite Run boundaries. The adapter does not substitute a saturation
  for a finite run. Extra auxiliary updates can still affect enclosing loops or
  nonconfluent choices, so full-program validation remains mandatory.

The original rule bodies are retained in **unscheduled private rulesets** at
their old declaration sites. Executable replacements and auxiliary tables are
installed after the original rule definitions, before input actions/runs.
This preserves original schema/compilation order; the private copies never
execute. The adapter rejects late rule definitions, includes, unresolved surface
rewrites, namespace collisions and global-variable capture. It supports the
expanded flat Pass-3 program; it is not an arbitrary `.egg` compiler.

## Result: correct integration, no useful reuse with this library

| Quantity | Measured result |
|---|---:|
| Frozen Analyze states | 5 |
| Selected marker schemas | 1 |
| Scheduled consumer rules rewritten | 6 |
| Physical rows in that marker | **0** |
| Successful consumer applications | **0** |
| Compiled direct publication actions | **0** |
| Full original rows/values/equality/visibility | Passed |
| Production Tiger selected DAG | Passed |

The surviving fragment combines `InIf(false, pred, inputs)` and
`InIf(true, pred, inputs)`. The six consumers are context-error rules in
`switch_rewrite`/`cicm`. In this workload the marker is empty. Some rules are
reported with zero matches; unreported rules remain null in the ledger.
`shared.egg` and `published.egg` are byte-identical here, because there is no
admitted direct publisher. **This does not demonstrate producer reuse, Cell
coverage of the hot queries, or a Cell speedup.**

With the same retained-checkpoint/clone execution protocol, the current native
control took 22.442933 s and the final generated source 22.186437 s. These are
single runs, not an average. This small difference gets no Cell credit; there
were no successful consumers. The older unchanged native control (24.924421 s)
was reused for the exact graph and extraction checks, not presented as a new run.
Setup, compilation, graph comparison and Tiger are outside these inference times.

The existing five-state library does not supply an admitted common spine for
the active expensive terms queries under this source adapter's fully-keyed,
immutable-table contract. Adding more marker rows would not fix that coverage
problem. The actual per-marker/consumer evidence is in `results.json`; the bench
now emits `source-marker-ledger.json` automatically.

## Reproduction through the bench skill

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py marker-pass3 \
  --analysis experiments/analyze_library_credit/library \
  --reuse-controls /path/to/verified/pass3-compact-layout \
  --output out/pass3-source-marker-new
```

`--variant shared` disables direct publishers; `--variant original` explicitly
refreshes the unmodified native control when necessary. `--compiled DIR` reuses
generated source after checking input, library and source fingerprints. `--profile`
is a separate counter run and not speedup evidence. `--no-extract` explicitly
postpones Tiger. Normal execution returns nonzero if either the full-graph or
extraction check fails. It does not enable this mode by default elsewhere.

The output contains `program/shared.egg`, `program/published.egg`, the MIP and
binding evidence, plus `run/report.json`, graphs, Tiger results and the source
marker ledger. Final source hash:

`76c977ab6958a6a742d75696a04d392f2ebfdc31e3c17306f2164faf05da6935`

## Failures retained and corrected

The first version counted four `ivt-analysis` consumers that are not scheduled
in Pass 3. It maintained 13,341 irrelevant marker rows. These are excluded by
the schedule-aware compiler, not credited as reuse.

Early conversion variants also failed full graph and Tiger checks. The initial
benchmark itself directly executed its setup object, unlike the established
control, which runs a retained clone; even the unmodified input then differed.
That protocol was corrected and checked against the historical original graph.
Installing auxiliary tables amid original declarations was also removed. The
final late-installation/schedule-aware source passes both gates. Failed variants
are not used for speedup claims, and no original table is omitted to make a
failed comparison pass.

Small native regressions cover finite runs, a producer and consumer in the same
Run, later inputs, mutable numerical guards, union-enabled tuples, correlated
binding, subsumable tables, combined rulesets and late global-variable capture.
