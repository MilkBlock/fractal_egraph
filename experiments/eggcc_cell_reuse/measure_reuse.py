#!/usr/bin/env python3
"""Measure the cell-reuse / boundary-key repetition rate on a captured eggcc trace.

This is a read-only measurement over the artifacts the native analyzer already
writes (`observed-transitions.json`, `history.json`). It does not run or modify
the analyzer.

Definitions used here (all taken from the analyzer's own records):

* **member / apply**  one committed rule application = one *cell*.
* **join key**         `(rule, reader slot, fact id)`: the rule that was applied,
  the LHS position the fact was read at, and the fact itself. Two occurrences
  with the same key materialize the same fact at the same join position, so the
  second one is a *repeat*, not new evidence.
* **effect fingerprint** `(rule, sorted inserted_facts)`: occurrences with the
  same fingerprint wrote exactly the same rows.
* **reuse rate**       share of occurrences that sit on a key appearing more
  than once: `sum(k-1)/N`.
"""
import argparse
import collections
import json
import pathlib

p = argparse.ArgumentParser()
p.add_argument('run', type=pathlib.Path)
a = p.parse_args()
root = a.run.resolve()

sc = json.loads((root / 'live' / 'analysis.json').read_text())['saturated_rule_composition']
obs = json.loads((root / 'live' / 'observed-transitions.json').read_text())
tr = obs['transitions']

print(f"run              {root}")
print(f"logical matches  {sc['cs'].get('scope','')[:0] or ''}"
      f"{json.loads((root / 'live' / 'analysis.json').read_text())['summary']}")
print(f"jobs             {len(sc['jobs'])}  states={dict(collections.Counter(j['state'] for j in sc['jobs']))}")
print(f"observed applies {obs['count']}")

kinds = collections.Counter(t['kind'] for t in tr)
print(f"kinds            {dict(kinds)}")
print(f"rules            {len(set(t['rule'] for t in tr))}")
print(f"rulesets         {dict(collections.Counter(t['ruleset'] for t in tr).most_common(8))}")

n_facts = sum(len(t['inserted_facts']) for t in tr)
n_union = sum(len(t['union_effects']) for t in tr)
n_upd = sum(1 for t in tr if t['updated_rows'] is True)
n_ext = sum(len(t['external_facts']) for t in tr)
print(f"inserted facts   {n_facts}   union effects {n_union}   updated rows {n_upd}   external facts {n_ext}")

def reuse(counter, n, what):
    distinct = len(counter)
    rep = sum(v - 1 for v in counter.values())
    print(f"\n{what}")
    print(f"  occurrences       {n}")
    print(f"  distinct keys     {distinct}")
    print(f"  repeat share      {100 * rep / n:.1f}%  ({rep} of {n} occurrences repeat a key)")
    print(f"  mean multiplicity {n / distinct:.2f}   max {max(counter.values())}")
    return dict(occurrences=n, distinct=distinct, repeat_share=rep / n,
                mean_multiplicity=n / distinct, max_multiplicity=max(counter.values()))

# ------------------------------------------------- join-position keys
by_rule = collections.Counter(t['rule'] for t in tr)
by_rule_effect = collections.Counter((t['rule'], tuple(sorted(t['inserted_facts']))) for t in tr)
by_effect = collections.Counter(tuple(sorted(t['inserted_facts'])) for t in tr)
by_rule_union = collections.Counter((t['rule'], tuple(sorted(map(tuple, t['union_effects'])))) for t in tr)

r_rule = reuse(by_rule, len(tr), "key = rule identity alone")
r_eff = reuse(by_rule_effect, len(tr), "key = (rule, written rows)  <- restating the same effect")
r_effect_only = reuse(by_effect, len(tr), "key = written rows alone")
r_union = reuse(by_rule_union, len(tr), "key = (rule, union effects)")

# ------------------------------------------------- reader-slot keys
# `binding[i]` is the binding of LHS atom/reader position i; `inputs` are the
# ground facts it read. A Parent binding references an earlier member's output.
slot_keys = collections.Counter()
role_keys = collections.Counter()
slot_occ = 0
for t in tr:
    b = t['binding']
    ins = t['inputs']
    for i, entry in enumerate(b):
        if not isinstance(entry, dict):
            continue
        if 'Parent' in entry and isinstance(entry['Parent'], list) and len(entry['Parent']) == 2:
            member, out = entry['Parent']
            slot_occ += 1
            slot_keys[(t['rule'], i, member, out)] += 1
        elif 'Input' in entry:
            n = entry['Input']
            if n < len(ins):
                slot_occ += 1
                slot_keys[(t['rule'], i, 'ext', ins[n])] += 1
r_slot = reuse(slot_keys, slot_occ, "key = (rule, reader slot, producer member/output)")

# ------------------------------------------------- fact popularity across whole trace
fact_use = collections.Counter()
for t in tr:
    for f in t['inputs']:
        fact_use[f] += 1
print(f"\nfact reads        {sum(fact_use.values())} reads over {len(fact_use)} distinct facts, "
      f"max multiplicity {max(fact_use.values())}")
top = fact_use.most_common(5)
print(f"  hottest facts   {top}")
r_fact = reuse(fact_use, sum(fact_use.values()), "key = fact id (any position)")

# ---------------------------------------------------- per-rule hotness
print("\nper-rule members (top 12)")
for rule, c in by_rule.most_common(12):
    print(f"  {c:5d}  rule {rule}")

out = dict(run=str(root), jobs=len(sc['jobs']),
           states=dict(collections.Counter(j['state'] for j in sc['jobs'])),
           observed_applies=obs['count'], kinds=dict(kinds),
           distinct_rules=len(by_rule), inserted_facts=n_facts,
           union_effects=n_union, updated_rows=n_upd, external_facts=n_ext,
           reuse_rule_identity=r_rule, reuse_rule_and_written_rows=r_eff,
           reuse_written_rows_only=r_effect_only, reuse_rule_and_union=r_union,
           reuse_rule_slot_producer=r_slot, reuse_fact_reads=r_fact,
           reader_slots_observed=slot_occ,
           members_by_rule={str(k): v for k, v in by_rule.most_common()})
(root / 'reuse-rate.json').write_text(json.dumps(out, indent=2) + '\n')
print(f"\nwrote {root / 'reuse-rate.json'}")
