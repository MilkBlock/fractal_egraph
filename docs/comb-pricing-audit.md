# Audit of the packing/granularity diagnosis

This is a read-only audit of the four saved native Math6 analysis outputs in
`out/adaptive-reuse-final`, compared with the probe at `probe/packing` commit
`dbb7bcc`. No tier0 execution, kernel changes, or full repository copy was needed.
The main checkout's analyzer and the probe worktree were not modified.

## Correct the observability denominator first

The original probe's `collect_reads` records read slots, while `summarize` builds
sets of `(member, output)` per Use. `total_interior = sum(len(set))` then counts
*distinct ports*, not read occurrences. Dividing this by `len(reads)` mixes units.

| Arm | Cross-Use read slots | Interior read slots | Correct fraction | Distinct interior ports |
|---|---:|---:|---:|---:|
| eager | 3236 | 2932 | 90.61% | 1099 |
| probation | 3191 | 2862 | 89.69% | 1067 |
| incremental | 2833 | 2618 | 92.41% | 993 |
| full | 2807 | 2595 | 92.45% | 992 |

Thus 35.3% is not the corrected read fraction. The online counter and final-cover
counter do measure different times/covers, but that does not make their numeric
difference an overestimate: the full final-cover fraction is higher here.
The native counter iterates binding slots, not context references.

The probe excludes templates with fewer than two active Uses from its closed
category. With that restriction none qualify. Including singletons gives T148
in eager and T162 in full. This supports neither a universal theorem that no
closed template exists nor the claim that no conditional/specialized packing
scheme could exist.

Moreover, full's 2595 interior reads consist of 1105 `var` outputs, 458 `column`
outputs and 1032 `row` outputs. A non-root apply's output may be an inherited
binding value, not a newly created interior e-node. This probe measures member-port
observability, not an independently established set of deletable tier0 nodes.
General packing safety needs a more precise materialization and observation model.

## Pricing: representation limitation, not established arithmetic bug

`old_cost` should use the already compressed parts when deciding whether to
replace the current cover. Comparing against their original raw size would count
savings already achieved. Charging the flat definition is also consistent with the
current stored flat `Pattern`; `Template.atoms` does not replace that storage or
encode the complete boundary-wiring and alias substitution tables by itself.

There is nevertheless a real representational limitation: hierarchical recipes
exist, but large definitions still retain their flat matching/verification form.
A new representation could improve this. Simply discounting the current score
would not implement that representation.

The audit computes one **optimistic fixed-cover scenario**, not a functioning
codec or an admission replay:

- flat leaf step cost is the existing model;
- a child-template recipe call costs `1 + member_mapping_length`;
- boundary-wire and alias-substitution tables are deliberately omitted;
- choose the cheaper local flat/recipe definition, and charge its transitive child
  definitions once (including children not currently active);
- replace per-instance member-list cost with part-list cost, assuming implicit
  expansion replaces the flat member list.

Local definition choice is not a globally optimized dictionary installation plan.
Historical instances, indexes and evidence remain in the real implementation.

| Arm | Current selected wiring + active dictionary | Optimistic scenario |
|---|---:|---:|
| eager | 28901 | 26663 |
| probation | 27939 | 26741 |
| incremental | 27854 | 26810 |
| full | 27169 | 26419 |

Full's difference is 750 model units (2.76%), after charging seven additional
child definitions. This is not observed memory savings. T77, a six-member active
template, has flat cost 108 and recipe skeleton score 29, but 29 excludes its child
T53's installation and substitution interface. It cannot safely replace 108 in
runtime accounting without changing the representation.

## What the snapshots do and do not establish

Eager contains templates through size 16, but active templates stop at size 7;
full contains sizes through 8 and active ones through 6. Most eager size 11–16
entries have just one observation. A cheaper encoding alone cannot supply an
unobserved repeated match.

The following are plausible mechanisms supported by source, not isolated causal
proofs: exact whole-region keys, probation churn, lexicographic candidate truncation,
whole-frontier growth, and a re-cut operation that only exposes existing parts.
The builder can still merge during later `push` calls. Eager is not a clean ablation
of MAX_MEMBERS alone; "raising budgets cannot help" is stronger than these data.
LRU chooses the oldest entry, not the largest one; size affects space pressure and
credit requirements, but does not by itself establish which entry is evicted first.

Saved JSON lacks rejected candidates' prices, evicted probation records and the
historical sequence of cover changes. In rotation cases `Use.parts` also does not
record every displaced old Use and spill. Therefore it cannot answer exactly
"how many large candidates would become profitable/admitted" under new pricing.
That needs the same candidate stream scored by both schemes, with compact
old/new/definition/spill/interface cost records. Relaxing equality checks would
also require explicit parameterization and constraints, not dropping alias,
requirement or effect distinctions.

## Reproduce

```sh
python3 tools/probe_comb_pricing.py --base out/adaptive-reuse-final --output docs/comb-pricing-audit.json
python3 tools/test_probe_comb_pricing.py
```

The result includes input SHA256 digests, size distributions and sample template
recipes. Tests independently distinguish duplicate reads from distinct ports,
retain closed singletons, and charge an inactive dependency definition rather than
pretend that a child-template call is free.
