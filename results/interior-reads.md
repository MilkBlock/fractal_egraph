# Interior-read probe: is the "pack a composition into an opaque constructor" core non-empty?

**Scope.** Read-only probe over recorded `analysis.json` output. No rules are rewritten,
no tier0 execution takes place, and nothing here measures engine time. Numbers are counts
of structural references, not bytes and not speedups.

## Question

The packing scheme (declare a constructor for a discovered composition, solve on the packed
form, expand once at the end) only saves work if the composite's interior members stop being
matched. Adding a constructor is monotone and saves nothing; the saving requires
`:subsume`, which is non-monotone and removes the interior rows from matching — and, per
`src/extract.rs:421,468`, from extraction as well.

The cheapest sound variant needs **no** consumer rewrites: pick a template whose interior
members are never observed from outside, compose its rules, and let the intermediate never
exist. That is a whole-program property, but its **necessary** condition is observable in the
recorded history:

> does any consumer *outside* a Use read a non-root member's output?

If that necessary condition fails for every template, the sufficient condition certainly
fails, and this variant is dead without needing a rule-level domination proof.

## Method

`tools/probe_interior_reads.py`. Definitions encoded to match `comb_reuse::push`:

- `owner[event] = (use, member)` reconstructed from the active cover in `reuse.roots`
- a read exists when residual `j` has a binding slot `ResidualPort{event: p, output: o}` with
  `owner[p] = (u, m)`
- the read is **external** when `j ∉ members(u)` (edges inside a composite are excluded;
  `push` never counts them, because the Use did not exist when `j` was pushed)
- the read is **interior** when `m ≠ pattern.root`
- `closed(u)` = `u` has no external interior read

## Results (Math6, 1529 events, four policy arms)

| arm | active Uses | used templates | closed Uses | **closed templates** | interior / reads through a Use |
|---|---:|---:|---:|---:|---:|
| eager | 251 | 89 | 34 (13.5%) | **0** | 1099 / 3236 (34.0%) |
| probation | 254 | 66 | 32 (12.6%) | **0** | 1067 / 3191 (33.4%) |
| incremental | 240 | 53 | 33 (13.8%) | **0** | 993 / 2833 (35.1%) |
| full | 243 | 45 | 34 (14.0%) | **0** | 992 / 2807 (35.3%) |

Cover conflicts 0 in every arm (the cover is a genuine disjoint partition).

## Verdict

**The no-projection core is empty.** In all four arms, closed *instances* exist (~13%) but no
template has all of its active instances closed. The closed instances live in only 7 of 45
templates in the full arm, and each of those templates is majority-open. So the property is
not "rare at the granularity where rules live" — it is **absent** there. Closedness is an
accident of a particular history (that instance's interior happened not to be read), not a
property of the pattern a rule could be written against.

The stronger reading: the reason no template is closed is that interior members are
*routinely* consumed by outside rules. That is evidence that the interior is observable at
rule level for essentially every template — it undercuts the premise of the projection
variant too, not just the no-projection one.

## Correction to the runtime counter

`stats.interior_port_reads / stats.continuations_through_use = 2759 / 3061 = 90%` overstates
the final structure. That counter accumulates while the cover changes, and re-cuts deactivate
Uses, so it counts reads through Uses that are no longer active. Against the **final** cover
the figure is 992 / 2807 = **35.3%**. The probe's ratio is stable at 33–35% across arms.

(The probe is not an exact reproduction of the counter and is not expected to be: the counter
is a push-time accumulation over a moving cover, the probe is a snapshot of the final cover.)

## What survives for the projection variant

Projection surface = distinct interior ports read externally, per template:

| ports | templates |
|---:|---:|
| 0 | 1 (a singleton template with 1 active Use — degenerate) |
| 2 | 1 |
| 3 | 17 |
| 4–6 | 8 |
| 8–12 | 17 |
| 21 | 1 |

Best candidates by instance count, full arm:

| template | active Uses | closed | open | members | projection ports |
|---|---:|---:|---:|---:|---:|
| T2 | 60 | 19 | 68% | 2 | 3 |
| T0 | 16 | 4 | 75% | 2 | 3 |
| T18 | 12 | 4 | 67% | 3 | 9 |
| T83 | 11 | 3 | 73% | 3 | 3 |

T2 and T83 have small projection surfaces (3), so the *encoding* cost of the projection
variant is not the blocker for them. The blocker is the one this probe does not measure: every
consumer atom matching an interior port must be rewritten, and that rewrite is a whole-program
change whose completeness obligation is not settled by any finite history.

## Caveats

- One workload (Math6) and four arms of one builder. A different program with more
  single-consumer intermediates could behave differently.
- "Closed" here is history-level. It can only ever **refute** the rule-level property, never
  establish it. That is enough for the negative verdict above.
- The probe reads the final cover, so it does not attribute reads to the cover state at the
  time they occurred.
