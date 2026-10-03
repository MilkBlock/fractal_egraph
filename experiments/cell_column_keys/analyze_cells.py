#!/usr/bin/env python3
"""Two read-only measurements over one analysis run's cells.

1. Boundary-probe ranking: which read positions are genuine cross-cell join
   keys, how often the same key value repeats, and how many distinct symbolic
   keys each position carries. This is the "key side" of choosing a narrow
   driver column; it is computable from the cell records alone.

2. Cell extent vs closure size: whether the cells that fail to close are the
   *deep* ones (a depth limit) or the *wide* ones (a fact-budget limit).

Reads `<run>/analysis.json`; changes nothing.
"""
import argparse
import collections
import json
import pathlib
import re
import statistics

p = argparse.ArgumentParser()
p.add_argument('run', type=pathlib.Path)
a = p.parse_args()
d = json.loads((a.run.resolve() / 'analysis.json').read_text())
sc = d['saturated_rule_composition']
jobs = sc['jobs']

# ---------------------------------------------------------------- key side
role_occ = collections.Counter()
role_vals = collections.defaultdict(set)
role_bare = collections.Counter()
boundary_ports = 0
for j in jobs:
    o = j.get('origin') or {}
    toks = {v['token']: v for v in (o.get('symbolic_values') or [])}
    for m in o.get('comb_members') or []:
        b = m.get('binding') or []
        roles = m.get('input_roles') or []
        al = m.get('aliases') or []
        for i, entry in enumerate(b):
            if not isinstance(entry, dict) or 'Input' not in entry:
                continue
            port = entry['Input']
            if port >= len(al):
                continue
            role = roles[i] if i < len(roles) else '?'
            tok = al[port]
            term = (toks.get(tok) or {}).get('term', '?')
            role_occ[role] += 1
            role_vals[role].add(tok)
            boundary_ports += 1
            if re.fullmatch(r'\(RipenInput \d+\)', term):
                role_bare[role] += 1

ranking = []
for role, n in role_occ.items():
    distinct = len(role_vals[role])
    ranking.append(dict(role=role, occurrences=n, distinct_keys=distinct,
                        hit_multiplicity=n / distinct,
                        bare_boundary_share=role_bare[role] / n))
ranking.sort(key=lambda r: -r['hit_multiplicity'])

# ------------------------------------------------------- extent vs closure
members = collections.defaultdict(list)
facts = collections.defaultdict(list)
deepest = []
for j in jobs:
    o = j.get('origin') or {}
    m = len(o.get('comb_members') or [])
    g = (j.get('observation') or {}).get('growth') or {}
    members[j['state']].append(m)
    if g.get('after_facts') is not None:
        facts[j['state']].append(g['after_facts'])
    deepest.append(dict(members=m, state=j['state'],
                        after_facts=g.get('after_facts')))

extent = {s: dict(n=len(v), min=min(v), median=statistics.median(v), max=max(v))
          for s, v in members.items()}
closure = {s: dict(n=len(v), min=min(v), median=statistics.median(v), max=max(v))
           for s, v in facts.items()}

out = dict(run=str(a.run.resolve()), jobs=len(jobs),
           states=dict(collections.Counter(j['state'] for j in jobs)),
           boundary_ports=boundary_ports,
           boundary_probe_ranking=ranking,
           cell_extent_by_state=extent,
           closure_facts_by_state=closure,
           deepest_cells=sorted(deepest, key=lambda x: -x['members'])[:10])

print(f"jobs={len(jobs)} states={out['states']} boundary_ports={boundary_ports}")
print(f"\n{'role':44s} {'occ':>5s} {'distinct':>8s} {'mult':>6s} {'bare%':>6s}")
for r in ranking[:12]:
    print(f"{r['role'][:44]:44s} {r['occurrences']:5d} {r['distinct_keys']:8d} "
          f"{r['hit_multiplicity']:6.2f} {100*r['bare_boundary_share']:5.0f}%")
print("\ncell extent (members per cell) by state:")
for s, v in extent.items():
    print(f"  {s:20s} n={v['n']:3d} min={v['min']} median={v['median']} max={v['max']}")
print("closure facts by state:")
for s, v in closure.items():
    print(f"  {s:20s} n={v['n']:3d} min={v['min']} median={v['median']} max={v['max']}")
print("deepest cells:", out['deepest_cells'][:6])
(a.run.resolve() / 'cell-column-keys.json').write_text(json.dumps(out, indent=2) + '\n')
print(f"\nwrote {a.run.resolve() / 'cell-column-keys.json'}")
