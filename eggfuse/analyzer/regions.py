"""Name-independent dependency regions and shared, correlated Spine programs.

A cached Spine has at most one row per anchor witness: every subsequent table
read has a complete key. Branching joins and primitive guards remain in the
exact consumer, so a mixed ruleset never needs one whole-ruleset certificate.
"""
import copy
from collections import defaultdict
from eggfuse.ir.recipe import build, validate, digest, require

from eggfuse.ir.regions import SCHEMA, query_order


def analyze(recipe, marker_names=()):
    core = validate(recipe)
    marker_names = sorted(set(marker_names))
    tables = {t['name']: t for t in core['tables']}
    nodes = []; node_ids = {}; rules = []; identities = {}; sets = []
    for si, rs in enumerate(core['rulesets']):
        uses = []
        for ordinal, rule in enumerate(rs['rules']):
            identity = (rule.get('source_set',rs['name']),rule['name'])
            if identity in identities:
                uses.append(identities[identity]); continue
            rid = len(rules); identities[identity] = rid; uses.append(rid)
            body = rule['body']; n = sum('table' in a['call'] for a in body)
            variants = []
            for pivot in (range(n) if rule['seminaive'] and n else [None]):
                order = query_order(body,pivot,marker_names)
                known = set(); prefix = []; names = {}; sorts = []; parent = None
                for a in order:
                    if 'table' not in a['call']: break
                    # Keep fanout in the exact consumer; do not guess a bound or
                    # truncate alternatives to make a cache artificially small.
                    if prefix and not all('literal' in x or x['var'] in known for x in a['args'][:-1]): break
                    args = []
                    for x in a['args']:
                        if 'literal' in x: args.append(copy.deepcopy(x))
                        else:
                            if x['var'] not in names:
                                names[x['var']] = len(names); sorts.append(x['sort'])
                            args.append({'slot':names[x['var']]}); known.add(x['var'])
                    atom = {'table':a['call']['table'],'args':args}
                    signature = digest(dict(parent=parent,atom=atom,sorts=sorts))
                    if signature not in node_ids:
                        node_ids[signature] = len(nodes)
                        reads = sorted(set(([atom['table']] if parent is None else nodes[parent]['reads']+[atom['table']])))
                        nodes.append(dict(id=len(nodes),parent=parent,atom=atom,sorts=copy.deepcopy(sorts),
                                          tables=len(prefix)+1,reads=reads,width=len(sorts)+2*(len(prefix)+1)))
                    parent = node_ids[signature]; prefix.append(a)
                variants.append(dict(pivot=pivot,order=[a['_ordinal'] for a in order],
                    prefix=[a['_ordinal'] for a in prefix],table_atoms=[a['_table_atom'] for a in prefix],
                    node=parent if len(prefix)>1 else None,slots=names if len(prefix)>1 else {}))
            f=recipe['rules'][recipe['rulesets'][si]['rule_ids'][ordinal]]['footprint']
            rules.append(dict(id=rid,recipe_rule=recipe['rulesets'][si]['rule_ids'][ordinal],identity=list(identity),variants=variants,reads=f['read_tables'],writes=f['write_tables'],operations=f['operations'],union_sorts=f['union_sorts']))
        sets.append(dict(name=rs['name'],rules=uses))
    # Prune single-atom plans which have no consumer or descendant. No row ×
    # rule allocation: metadata is static; actual tuples are allocated on GPU.
    needed=set()
    def use(n):
        while n is not None and n not in needed:needed.add(n);n=nodes[n]['parent']
    for r in rules:
        for v in r['variants']:use(v['node'])
    remap={old:new for new,old in enumerate(sorted(needed))}
    nodes=[dict(n,id=remap[n['id']],parent=remap.get(n['parent'])) for n in nodes if n['id'] in needed]
    for r in rules:
        for v in r['variants']:v['node']=remap.get(v['node'])
    # Keep effects and exact queries in Recipe IR; this is an optimization plan,
    # never a replacement set of handwritten rules.
    readers=defaultdict(set); writers=defaultdict(set)
    for r in rules:
        for t in r['reads']:readers[t].add(r['id'])
        for t in r['writes']:writers[t].add(r['id'])
    # SCCs describe mutually dependent rule regions; they never replace the
    # source control tree. Union may trigger indirect congruence/merge writes,
    # so analysis conservatively connects those producers to all readers.
    graph=[set() for _ in rules]
    dependencies=[]
    for t in tables:
        producers=set(writers[t]) | {r['id'] for r in rules if r['union_sorts']}
        if producers and readers[t]:
            dependencies.append(dict(table=t,producers=sorted(producers),consumers=sorted(readers[t])))
            for a in producers:graph[a].update(readers[t])
    reverse=[set() for _ in rules]
    for a,targets in enumerate(graph):
        for b in targets:reverse[b].add(a)
    seen=set();finish=[]
    for start in range(len(rules)):
        if start in seen:continue
        stack=[(start,False)]
        while stack:
            v,done=stack.pop()
            if done:finish.append(v);continue
            if v in seen:continue
            seen.add(v);stack.append((v,True))
            stack.extend((w,False) for w in sorted(graph[v],reverse=True) if w not in seen)
    seen=set();regions=[]
    for start in reversed(finish):
        if start in seen:continue
        group=[];stack=[start];seen.add(start)
        while stack:
            v=stack.pop();group.append(v)
            for w in sorted(reverse[v]):
                if w not in seen:seen.add(w);stack.append(w)
        regions.append(dict(id=len(regions),rules=sorted(group),
            rulesets=[s['name'] for s in sets if set(s['rules'])&set(group)],
            operations=sorted({o for rid in group for o in rules[rid]['operations']}),
            activation='original Run membership and read snapshot; region edges do not grant early execution'))
    routes=[dict(table=t,nodes=[n['id'] for n in nodes if t in n['reads']],consumers=sorted(readers[t]),
                 trigger='committed canonical row insertion/value/visibility change, including merge and rebuild') for t in tables]
    payload=dict(schema=SCHEMA,recipe_sha256=recipe['sha256'],scope=recipe['scope'],marker_names=marker_names,
        nodes=nodes,rules=rules,rulesets=sets,regions=regions,dependencies=dependencies,
        routes=routes,control=copy.deepcopy(recipe['control']),
        publication='one original control program in a shared GPU identity domain; internal Run read boundaries retained',
        storage='only actual correlated anchor tuples; dynamic packed arena with invalidation and reclamation',
        residual='all uncached atoms and guards retained; no inferred absence from training frequency')
    return dict(payload,sha256=digest(payload))


def verify(program,recipe):
    require(program == analyze(recipe,program.get('marker_names',[])), 'region plan differs from typed analysis')
    return program


def from_core(core,marker_names=()):
    return analyze(build(core),marker_names)
