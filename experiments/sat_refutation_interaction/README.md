# Does better Cell inference strengthen GPU refutation?

The experiment supports **complementary division of work**. It does not show
that CPU saturation makes the refuter intrinsically stronger on an unchanged
candidate. The distinction matters: a rejection rate can rise because the
queries sent to the GPU changed.

## Actual Pass-3 factorial experiment

Same saved raytrace Pass 3 input, all 547 original rules, one Rust binary and
one Swift worker, one run per configuration. Native RHS, ordinary Run boundaries,
selection policy and final graph checks are unchanged. All five graphs pass
the same saved same-commit CPU full-state reference. The pre-existing fused
native discrepancy remains; it is not relabelled as a pass.

`S=1` means both existing CPU Cell services: certified saturation handling and
complete incremental query-frontier reuse. `R=1` means the current empty-bucket
and key-fingerprint refuter. Domain intersection is off. A fifth run disables
ordinary Cell query reuse while retaining saturation handling, to separate
the two CPU services.

These runs enable atomic work counters. **Their seconds are diagnostic, not
uninstrumented speedup measurements.** This is a runtime service ablation,
not a change of Bake training data, library size, or certificate quality.

| CPU Cell services | GPU refutation | Live candidate row visits | Indexed Join stages |
|---|---|---:|---:|
| Off | Off | 789,963,008 | 229,531,003 |
| Off | On | 72,750,548 | 60,237,385 |
| On | Off | 762,210,024 | 194,029,242 |
| On | On | **48,033,034** | **26,650,658** |

These are repeated GPU operations, not distinct facts, matches, new mutations,
or total CPU+GPU time. The combined configuration reduces recorded candidate
visits by **93.92%** versus neither optimization. After refutation is already
on, the Cell services still remove **24,717,514 visits (33.98%)**.

The absolute savings are nearly additive, not superadditive. Let `V_sr` denote
the table's row visits. The interaction is

```text
I = V10 + V01 - V00 - V11
  = -3,035,470 visits.
```

The negative value means about 3.04 million saved visits overlap. It does not
negate complementarity: each still helps after the other is enabled. It does
rule out claiming a superadditive increase in refutation's absolute saving on
this workload.

## The apparent hit-rate improvement, with a proper denominator

A counted **seed** is a multi-atom query start after its anchor and currently
available primitive checks succeed, before negative lookahead. Each seed has
exactly one of three outcomes: immediately refuted; produces at least one
complete binding; or survives lookahead but ultimately produces no binding.
The last category may be eliminated by later constraints or delta ownership.
A productive seed need not cause a new mutation.

| CPU services, refutation on | Eligible seeds | Immediately refuted | Refutation rate |
|---|---:|---:|---:|
| None | 40,587,639 | 7,298,840 | **17.983%** |
| Saturation handling only | 38,166,162 | 7,189,573 | **18.838%** |
| Saturation + Cell query reuse | 13,278,476 | 7,144,255 | **53.803%** |

So the user's predicted aggregate rate increase is observed. But absolute
refutations decrease, while much more work disappears before the GPU query.
More refutations are not themselves the optimization objective.

Across each refutation on/off pair, eligible and productive seed counts are
identical. The filter removes search without removing successful bindings.
Per-rule counts sum to the global counters, and `refuted + productive <=
eligible` is checked on the device receipt and in the report tool.

## Controlling for which rules still reach the GPU

Among the **376 rules with seeds in both no-Cell and full-Cell runs**, use the
no-Cell seed counts as fixed weights. The rejection rates are:

```text
Same rule mix, before: 53.424741%
Same rule mix, after:  53.452506%
Difference:             0.027765 percentage points
```

This controls rule mix, not the identities of every seed within a rule. It is
not a matched-candidate causal estimate. Still, it shows that nearly all of
the aggregate 17.98% -> 53.80% change comes from changing which queries reach
the GPU, rather than a large within-rule increase in refutation power.

Full Cell services remove all GPU seeds for 24 rules: together they previously
generated 27,166,318 seeds but only 128,534 immediate rejections. Most of these
seeds were not productive either; they were hard for this particular *early*
filter to eliminate. Do not describe them all as known successful matches.

Saturation-only removes 11 rules' seeds; all 389 remaining active rules have
identical eligible/refuted/productive counts. Enabling ordinary Cell query
reuse removes another 13 rules, many computing `Expr-size`. For example:

```egg
(rule ((= expr (If pred inputs then else))
       (= sum (+ (Expr-size inputs)
                 (+ (Expr-size else)
                    (+ (Expr-size then) (Expr-size pred))))))
      ((set (Expr-size expr) (+ sum 1)))
      :ruleset always-run)
```

Without ordinary Cell query reuse this rule had 4,487,242 eligible GPU seeds
but only 338 early rejections. The Cell provider removes its GPU search by
answering its complete incremental query. It does not pass additional partial
bindings for that rule to the GPU. This is useful reuse, but is not evidence
that richer learned RipSatCells already strengthen boundary refutation.

## What can actually be proved

Fix a graph snapshot `G`, query `q`, candidate group `c`, and valid interface
versions. Let `M` be all real matching bindings in that group. Suppose two
levels of certified Cell knowledge supply sound *upper bounds*:

```text
M ⊆ U2 ⊆ U1.
```

Let `E` be the unchanged remaining external constraints. If `U1 ∩ E` is empty,
then `U2 ∩ E` is empty. A tighter upper bound can additionally eliminate cases
where `U1 ∩ E` was nonempty. This is the conditional monotonicity theorem the
proposed architecture would need.

For an approximate implementation, keep the old valid rejection tests while
adding tighter tests, or use nested overapproximations in fixed coordinates.
Replacing an old hash projection with an unrelated hash layout can lose a
previous rejection to a new collision; precision does not follow merely from
adding more feature coordinates to a different representation.

The theorem requires all of the following:

- The graph snapshot, candidate group and external constraints are fixed.
- Bounds include **all** possible internal bindings, not just observed positive
  examples or one chosen spine. Unknown coverage remains a fallback.
- Additional knowledge refines the bound, rather than silently changing the
  query population or selecting one binding alternative.
- Union, insertion, updates and visibility changes renew or invalidate the
  relevant interface certificate before it is used.

This is stronger than “a larger Bake library.” More certified coverage is useful,
but it need not produce a nested upper bound on the same candidate population.

More *facts* also do not monotonically increase impossibility. In a fixed
positive query `P(x) ∧ Q(x)`, a state containing only `P(a)` has no match;
CPU inference that adds `Q(a)` makes the match possible. Union can likewise
enable matches. This is a change of the graph, not a refinement of knowledge
about one fixed snapshot.

Finally, a saturation certificate can prove **no new local effect** while many
LHS matches still exist. That is a valid reason to skip work, but must not be
called proof that the LHS cannot match.

## Current mechanism and the missing connection

`table_live.rs` calls `query_cells`, marks whole query indices as provided,
and sends only unclaimed queries to the GPU. The GPU refuter receives original
typed LHS wiring, partial bindings from its own Join, and complete native table
indexes/summaries. It currently receives no tighter Cell-specific binding upper
bound. In this full-pass path its GPU Cell library is null; the CPU certificate
provider is a separate component. Holding `G,q,c` **and the maintained index/
summary state** fixed, the refuter is independent of which Cell certificates
happen to exist on the CPU. Holding only the live graph fixed is insufficient:
retained stale positive bits and index rebase history can change a conservative
filter's precision without changing the live facts.

The useful demonstrated policy is therefore: handle locally answerable work
with Cell certificates, and send the residual search to the negative filter.
To claim stronger *per-candidate* refutation next, export a complete,
versioned Cell interface bound and test the same fixed candidate cohort with
and without that bound. Keep residual external cases; missing Bake coverage
cannot be interpreted as an empty domain.

## Reproduce / inspect without rerunning

All actual runs use the existing bench skill, `run --all-rules --no-extract`
with `CELL_SEARCH_COUNTERS=1` and saved CPU controls. Toggle `--joint-sat` for
S, `CELL_REFUTATION=0/1` for R; the middle group also sets
`CELL_DISABLE_CELL_QUERIES=1`.

The native/CPU controls from an existing joint run can now be reused for a
no-hook candidate too. The helper removes stale joint comparison fields and
still requires a validated reference, identical schedule/partition, and a fresh
full graph check. It does not carry an old joint verdict onto a new run.

Read the five saved runs and regenerate the analysis without compiling or
running inference:

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py interaction \
  --s0r0 out/sat-refutation-s0r0 --s0r1 out/sat-refutation-s0r1 \
  --s1r0 out/sat-refutation-s1r0 --s1r1 out/sat-refutation-s1r1 \
  --sat-only out/sat-refutation-sat-only-r1
```

`--output NEW.json` optionally saves the result. The tool verifies input,
binary, worker, schedule, switches, graph proof and counter accounting. Zero
remaining candidates yield an undefined rate, not a fabricated 0% or 100%.
Full provenance and removed/common-rule counts are in `results.json`.

The new profiling receipt is layout 13. Per-rule seed counters allocate three
words per query only in diagnostic mode; ordinary inference does not execute
their atomic updates. The compact executor's seed population is the scope of
these counters. No timing result from the counted runs is promoted to a speedup.

Validation: 25 Python workflow/report tests, 5 Rust benchmark/control-cache
tests, and 10 full-GPU native-oracle regressions pass. The latter enable both
refutation modes and seed diagnostics, including inserts, union, updates,
subsume, computed keys and backtracking. Five fresh full Pass-3 graph checks
pass against the same-commit reference. This change adds observation and report
tools; it does not change matching or saturation semantics, and does not
claim a new Tiger or pristine-upstream validation run.
