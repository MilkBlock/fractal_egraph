#!/usr/bin/env python3
"""Read-only cost/observability audit of saved Use dictionaries. No tier0 execution.
Recipe scores omit binding/alias substitution tables: they are optimistic scenarios,
not an implemented codec, observed savings, or a replay of rejected candidates.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path


def step_cost(step):
    return 4 + 3 * len(step['wiring']) + len(step['internal_parents']) + len(step['requires']) + len(step['effects'])


def audit(path):
    raw = path.read_bytes()
    r = json.loads(raw)['layers']['reuse']
    ts, us = r['templates'], r['uses']
    active = [p['Use'] for p in r['roots'] if 'Use' in p]
    owner = {}
    for u in active:
        for m, e in enumerate(us[u]['members']):
            assert e not in owner
            owner[e] = (u, m)
    reads = interior = 0
    ports = defaultdict(set)
    roles = Counter()
    for residual in r['residuals']:
        j = residual['event']
        for ref in residual['binding']:
            if 'ResidualPort' not in ref:
                assert 'UsePort' not in ref, 'This snapshot requires legacy reference resolution'
                continue
            p = ref['ResidualPort']
            hit = owner.get(p['event'])
            if hit is None:
                continue
            u, m = hit
            if j in us[u]['members']:
                continue
            reads += 1
            t = ts[us[u]['template']]['pattern']
            if m != t['root']:
                interior += 1
                ports[u].add((m, p['output']))
                role = r['schemas'][t['steps'][m]['schema']]['output_roles'][p['output']]
                roles[role.split(':')[0]] += 1
    by_template = defaultdict(list)
    for u in active:
        by_template[us[u]['template']].append(u)
    closed = [t for t, ids in by_template.items() if all(not ports[u] for u in ids)]
    flat = [sum(map(step_cost, t['pattern']['steps'])) for t in ts]
    recipes, deps = [], []
    for t in ts:
        score, children, covered = 0, set(), []
        for atom in t['atoms']:
            if 'Apply' in atom:
                m = atom['Apply']; covered.append(m)
                score += step_cost(t['pattern']['steps'][m])
            else:
                call = atom['Use']; child = call['template']
                assert len(ts[child]['pattern']['steps']) < len(t['pattern']['steps'])
                assert len(call['members']) == len(ts[child]['pattern']['steps'])
                covered.extend(call['members']); children.add(child)
                score += 1 + len(call['members'])
        assert sorted(covered) == list(range(len(t['pattern']['steps'])))
        recipes.append(score); deps.append(children)
    installed = set()

    def install(t):
        if t in installed:
            return
        installed.add(t)
        if recipes[t] < flat[t]:
            for child in deps[t]:
                install(child)

    for t in by_template:
        install(t)
    size_rows = []
    for size in sorted({len(t['pattern']['steps']) for t in ts}):
        ids = [i for i, t in enumerate(ts) if len(t['pattern']['steps']) == size]
        chosen = [i for i in ids if i in by_template]
        size_rows.append({'members': size, 'installed_templates': len(ids), 'active_templates': len(chosen),
                          'with_child_calls': sum(bool(deps[i]) for i in ids),
                          'optimistic_recipe_cheaper': sum(recipes[i] < flat[i] for i in ids),
                          'observed_sum': sum(ts[i]['observed'] for i in ids),
                          'templates_with_matches': sum(ts[i]['matches'] > 0 for i in ids)})
    # Fixed current cover: this does not claim to reconstruct pre-selection old_cost.
    member_saving = sum(len(us[u]['members']) - len(us[u]['parts']) for u in active)
    selected = r['stats']['selected_wiring_units']
    flat_dictionary = sum(flat[t] for t in by_template)
    hypothetical_dictionary = sum(min(flat[t], recipes[t]) for t in installed)
    return {
        'input_sha256': hashlib.sha256(raw).hexdigest(),
        'observability': {'active_uses': len(active), 'active_templates': len(by_template),
            'cross_use_read_slots': reads, 'interior_read_slots': interior,
            'interior_read_fraction': interior / reads if reads else None,
            'distinct_use_interior_ports': sum(map(len, ports.values())),
            'closed_uses': sum(not ports[u] for u in active),
            'closed_templates_including_singletons': closed,
            'closed_templates_at_least_two_uses': [t for t in closed if len(by_template[t]) >= 2],
            'interior_output_role_counts': dict(roles)},
        'granularity': size_rows,
        'fixed_cover_pricing': {'selected_wiring': selected, 'flat_active_dictionary': flat_dictionary,
            'current_selected_total': selected + flat_dictionary,
            'optimistic_recipe_dictionary_with_dependencies': hypothetical_dictionary,
            'additional_dependency_templates': len(installed - set(by_template)),
            'optimistic_member_to_part_saving': member_saving,
            'optimistic_selected_total': selected - member_saving + hypothetical_dictionary,
            'all_candidate_index_current': r['stats']['candidate_index_units']},
        'template_examples': [dict(template=t, members=len(ts[t]['pattern']['steps']), uses=len(by_template[t]),
            flat=flat[t], recipe_skeleton=recipes[t], dependencies=sorted(deps[t]))
            for t in sorted(by_template, key=lambda t: (-len(ts[t]['pattern']['steps']), t))[:12]],
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--base', type=Path, default=Path('out/adaptive-reuse-final'))
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    result = {'scope': __doc__, 'missing_evidence': 'Snapshots omit rejected candidate prices, evicted probation entries, and historical cover changes. No admission-flip count or causal attribution can be recovered from these snapshots.',
              'arms': {arm: audit(a.base / arm / 'analysis.json')
                       for arm in ['eager', 'probation', 'incremental', 'full']}}
    a.output.parent.mkdir(parents=True, exist_ok=True)
    a.output.write_text(json.dumps(result, indent=2) + '\n')
    for arm, r in result['arms'].items():
        print(arm, json.dumps({'observability': r['observability'], 'pricing': r['fixed_cover_pricing']}))


if __name__ == '__main__':
    main()
