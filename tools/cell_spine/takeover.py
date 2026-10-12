"""Compile certified ancestor views into consumers, without reviving small Summaries."""
import itertools
from collections import defaultdict
from .terms import actions, text, walk


def var(name):
    return dict(kind='var', name=name)


def call(head, args):
    return dict(kind='call', head=head, args=list(args))


def replace(e, bindings):
    if e['kind'] == 'var':
        return bindings.get(e['name'], e)
    if e['kind'] == 'call':
        return call(e['head'], [replace(a, bindings) for a in e['args']])
    return e


def fork(e, bindings):
    if text(e) in bindings:
        return bindings[text(e)]
    if e['kind'] == 'call':
        return call('Spine-'+e['head'], [fork(a, bindings) for a in e['args']])
    return e


def fact_text(f):
    return '(= '+' '.join(text(e) for e in f['args'])+')' if f['kind']=='eq' else text(f['expr'])


def remap_fact(f, bindings):
    return (dict(kind='eq',args=[replace(e, bindings) for e in f['args']]) if f['kind']=='eq'
            else dict(kind='fact',expr=replace(f['expr'],bindings)))


def remap_action(a, bindings):
    if a['kind'] in ('expr','let'):
        result=dict(a,expr=replace(a['expr'],bindings))
        if a['kind']=='let': result['name']=bindings.get(a['name'],var(a['name']))['name']
        return result
    return dict(a,args=[replace(e,bindings) for e in a['args']])


def normalize(body, head):
    """Unify variable equalities; do not erase constructor-existence checks."""
    links = {}
    def find(name):
        while name in links:
            name = links[name]
        return name
    for f in body:
        if f['kind']=='eq' and all(e['kind']=='var' for e in f['args']):
            a,b = sorted(find(e['name']) for e in f['args'])
            if a != b:
                links[b] = a
    subst = {name:var(find(name)) for name in links}
    body = [remap_fact(f, subst) for f in body]
    body = [f for f in body if not (f['kind']=='eq' and f['args'][0]==f['args'][1] and f['args'][0]['kind']=='var')]
    head = [remap_action(a,subst) for a in head]
    # Alpha-equivalent inherited rules share one declaration. Preserve readable
    # names in the emitted rule; use canonical names only as the dedup key.
    names = {}
    roots = [e for f in body for e in (f['args'] if f['kind']=='eq' else [f['expr']])]
    roots += [e for a in head for e in ([var(a['name']),a['expr']] if a['kind']=='let' else [a['expr']] if a['kind']=='expr' else a['args'])]
    for root in roots:
        for e in walk(root):
            if e['kind']=='var': names.setdefault(e['name'],var(f'v{len(names)}'))
    alpha_body = list(dict.fromkeys(fact_text(remap_fact(f,names)) for f in body))
    alpha_head = [remap_action(a,names) for a in head]
    key = tuple(alpha_body),tuple(dict.fromkeys(actions(alpha_head)))
    return list(dict.fromkeys(map(fact_text,body))), list(dict.fromkeys(actions(head))), key


def ancestors(direct):
    """Finite family-DAG closure. Cyclic dominance is not a retirement proof."""
    graph = defaultdict(set)
    for lens in direct:
        graph[lens['owner']].add(lens['child'])
    def reachable(start):
        seen, todo = set(), list(graph.get(start,()))
        while todo:
            v = todo.pop()
            if v not in seen:
                seen.add(v); todo.extend(graph.get(v,()))
        return seen
    reaches = {v:reachable(v) for v in set(graph)|{x for vs in graph.values() for x in vs}}
    blocked = [l for l in direct if l['owner'] in reaches[l['child']]]
    by_owner = defaultdict(list)
    for lens in direct:
        if lens not in blocked:
            by_owner[lens['owner']].append(lens)
    cache = {}
    def views(owner):
        if owner in cache:
            return cache[owner]
        found = {}
        for edge in by_owner[owner]:
            variants = [edge]
            for child in views(edge['child']):
                subst = dict(zip(child['owner_fields'],edge['args']))
                variants.append(dict(owner=owner, child=child['child'], owner_fields=edge['owner_fields'],
                    child_fields=child['child_fields'], args=[replace(e,subst) for e in child['args']],
                    evidence=edge['evidence']+child['evidence']))
            for lens in variants:
                found[lens['child'],tuple(map(text,lens['args']))] = lens
        cache[owner] = list(found.values())
        return cache[owner]
    return [l for owner in list(by_owner) for l in views(owner)], blocked


def lower(rules, lenses, supports, definitions):
    """Lift every Summary operand independently, including several covered parents."""
    by_child = defaultdict(list)
    retirement_pairs = {(l['owner'],l['child']) for l in lenses}
    for lens in lenses:
        by_child[f'Spine-Summary-{lens["child"]}'].append(lens)
    emitted, result, redundant = set(), [], 0
    for opt, rule in rules:
        operands = [(i,f['expr']) for i,f in enumerate(rule['body'])
                    if f['kind']=='fact' and f['expr']['head'].startswith('Spine-Summary-')]
        alternatives = [[None]+by_child[e['head']] for _,e in operands]
        for n, choices in enumerate(itertools.product(*alternatives)):
            body = list(rule['body'])
            for (index, old), lens in zip(operands,choices):
                if lens is None:
                    continue
                rename = {name:var(f'cover_{index}_{name}') for name in lens['owner_fields']}
                owner = call(f'Spine-Summary-{lens["owner"]}',rename.values())
                body[index] = dict(kind='fact',expr=owner)
                body.extend(dict(kind='eq',args=[a,replace(b,rename)]) for a,b in zip(old['args'],lens['args']))
            head = list(rule['head'])
            target = head[-1]['expr']
            fields, extra = supports[opt['target_family']]
            subst = dict(zip(fields,target['args']))
            head = head[:-1] + [dict(a,expr=replace(a['expr'],subst)) if a['kind']=='expr'
                               else dict(a,args=[replace(e,subst) for e in a['args']]) for a in extra] + head[-1:]
            covered, implied = entailed(body,head,definitions,lenses)
            if any(choices) and covered and all(implied):
                redundant += 1
                continue
            head = [a for a,old in zip(head[:-1],implied) if not old] + head[-1:]
            # Retire directly matched parents in the SAME RHS as the new Cell.
            # An inherited operand belongs to a possibly larger/incomparable
            # owner; do not retire that owner or manufacture a missing small row.
            for (_, old), lens in zip(operands,choices):
                family = int(old['head'].removeprefix('Spine-Summary-'))
                if lens is None and (opt['target_family'],family) in retirement_pairs:
                    head.append(dict(kind='expr',expr=call('subsume',[old])))
            body_text, head_text, key = normalize(body,head)
            if key in emitted:
                continue
            emitted.add(key)
            name = opt['id'] if not any(choices) else f'{opt["id"]}_cover_{n}'
            emitted_rule = {**opt, 'id':name, 'base':opt['id'], 'inherited':any(choices),'body':body_text,'head':head_text,
                'rule':f'(rule ({" ".join(body_text)}) ({" ".join(head_text)}) :ruleset Spine-Run :name "{name}")'}
            emitted_rule['pre_inheritance_cost'] = {k:emitted_rule.pop(k) for k in
                ('search_atoms','update_actions','lookup_terms','write_terms','union_actions','equality_guards') if k in emitted_rule}
            result.append(emitted_rule)
    retire = []
    for i,lens in enumerate(lenses):
        owner = text(call(f'Spine-Summary-{lens["owner"]}',map(var,lens['owner_fields'])))
        child = text(call(f'Spine-Summary-{lens["child"]}',lens['args']))
        retire.append(f'(rule ({owner} {child}) ((subsume {child})) :ruleset Spine-Run :name "retire_{i}")')
    return result, retire, redundant


def entailed(body, head, definitions, lenses):
    """Finite congruence proof that this consumer would only republish existing data.

    Only rows guaranteed by the generated canonical constructors/support recipes
    count as materialized. A cached semantic closure alone is insufficient.
    """
    nodes, ids, parent, physical = [], {}, [], []
    def add(e):
        key = text(e)
        if key in ids: return ids[key]
        children = [add(a) for a in e.get('args',[])]
        i = len(nodes); ids[key] = i; nodes.append((e,children)); parent.append(i)
        return i
    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]; i = parent[i]
        return i
    def union(a,b):
        a,b = find(a),find(b)
        if a==b: return False
        parent[b]=a; return True
    def present(e):
        for sub in walk(e):
            if sub['kind']=='call': physical.append(add(sub))
    summaries = []
    for f in body:
        roots = f['args'] if f['kind']=='eq' else [f['expr']]
        for e in roots: add(e); present(e)
        if f['kind']=='eq': union(add(roots[0]),add(roots[1]))
        elif roots[0]['head'] in definitions:
            summaries.append(roots[0])
    for e in summaries:
        d = definitions[e['head']]
        bindings = dict(zip(d['fields'],e['args']))
        for i,root in enumerate(d['roots']):
            union(add(e['args'][i]),add(replace(root,bindings)))
        for i,j in d['aliases']: union(add(e['args'][i]),add(e['args'][j]))
        for term in d['materialized']: present(replace(term,bindings))
    target = head[-1]['expr']
    covers = []
    for e in summaries:
        if e['head']==target['head']: covers.append(e)
        for lens in lenses:
            if e['head']==f'Spine-Summary-{lens["owner"]}' and target['head']==f'Spine-Summary-{lens["child"]}':
                binding = dict(zip(lens['owner_fields'],e['args']))
                covers.append(call(target['head'],[replace(a,binding) for a in lens['args']]))
    for e in covers: add(e)
    for a in head:
        for e in ([a['expr']] if a['kind']=='expr' else a['args']): add(e)
    while True:
        found, changed = {}, False
        for i,(e,children) in enumerate(nodes):
            if e['kind'] != 'call': continue
            key = e['head'],tuple(find(c) for c in children)
            if key in found: changed = union(i,found[key]) or changed
            else: found[key] = i
        if not changed: break
    def same(a,b): return find(ids[text(a)])==find(ids[text(b)])
    covered = any(all(same(a,b) for a,b in zip(e['args'],target['args'])) for e in covers)
    available = {(nodes[i][0]['head'],tuple(find(c) for c in nodes[i][1])) for i in physical}
    implied = []
    for action in head[:-1]:
        roots = [action['expr']] if action['kind']=='expr' else action['args']
        known = action['kind'] != 'union' or same(*roots)
        for root in roots:
            for e in walk(root):
                if e['kind']=='call':
                    i=ids[text(e)]
                    if (e['head'],tuple(find(c) for c in nodes[i][1])) not in available: known = False
        implied.append(known)
    return covered, implied
