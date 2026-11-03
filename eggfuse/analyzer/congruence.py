"""Conditional, producer-local congruence proofs from typed constructor wiring.

No terminal egraph or observed alias partition is trusted. Equal constructor
arguments imply equal results. Repeated conditions share one test, including
through nested constructors; every unfolded constructor must still be present.
Unknown/cross-instance congruences remain the ordinary repairer's responsibility.
"""
from collections import defaultdict
from itertools import combinations
import json

SCHEMA = 'eggfuse-local-congruence/v1'

def key(a):
    return json.dumps(a, sort_keys=True, separators=(',', ':'))

def build(recipe):
    tables = {t['name']: t for t in recipe['tables']}
    templates = []
    for rid, rule in enumerate(recipe['rules']):
        effects = rule['effects']
        copies = {e['dst']: e['arg'] for e in effects if e['op'] == 'copy'}
        constructors = {e['dst']: i for i, e in enumerate(effects)
                        if e['op'] == 'let' and tables.get(e['call'].get('table'), {}).get('kind') == 'constructor'}
        def norm(a):
            while 'slot' in a and a['slot'] in copies: a = copies[a['slot']]
            return a
        memo = {}
        def compare(a, b):
            a, b = norm(a), norm(b)
            ka, kb = key(a), key(b)
            if ka > kb: a, b, ka, kb = b, a, kb, ka
            if ka == kb: return {}, set()
            if (ka, kb) in memo: return memo[ka, kb]
            ia, ib = constructors.get(a.get('slot')), constructors.get(b.get('slot'))
            if ia is not None and ib is not None and effects[ia]['call'] == effects[ib]['call']:
                guards, required = {}, {ia, ib}
                for x, y in zip(effects[ia]['args'], effects[ib]['args']):
                    result = compare(x, y)
                    if result is None:
                        memo[ka, kb] = None
                        return None
                    gs, rs = result; guards.update(gs); required.update(rs)
                result = guards, required
            elif 'literal' in a and 'literal' in b:
                result = None  # Unequal exact constants cannot establish this proof.
            else:
                result = {(ka, kb): [a, b]}, set()
            memo[ka, kb] = result
            return result
        # Intern structural shapes bottom-up; do not expand a shared DAG into a
        # tree or compare every pair of same-sort nodes at different depths.
        # Cross-shape equalities still go through ordinary repair.
        shapes, shape_cache = {}, {}
        def shape(a):
            a = norm(a); symbol = key(a)
            if symbol in shape_cache: return shape_cache[symbol]
            op = constructors.get(a.get('slot'))
            if op is not None:
                e = effects[op]
                signature = ('constructor', e['call']['table'], tuple(shape(x) for x in e['args']))
            elif 'slot' in a:
                signature = ('parameter', key(rule['slots'][a['slot']]['sort']))
            else: signature = ('literal', symbol)
            index = shapes.setdefault(signature, len(shapes))
            shape_cache[symbol] = index
            return index
        by_table = defaultdict(list)
        for dst, op in constructors.items(): by_table[shape({'slot': dst})].append(op)
        groups = {}
        for ops in by_table.values():
            for left, right in combinations(ops, 2):
                result = compare({'slot': effects[left]['dst']}, {'slot': effects[right]['dst']})
                if result is None: continue
                guards, required = result
                ordered = [guards[k] for k in sorted(guards)]
                group = groups.setdefault(key(ordered), {'guards': ordered, 'merges': []})
                group['merges'].append({'left': left, 'right': right, 'requires': sorted(required)})
        if groups: templates.append({'rule': rid, 'groups': list(groups.values())})
    return {'schema': SCHEMA, 'templates': templates}
