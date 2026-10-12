"""Consumer-facing views of certified Cell closures; no operator-family laws.

Only constructors read by compiled consumers get views. A Cell publishes these
rows using interface handles and one constructor witness per needed internal
value, not every alternate constructor of that value. Native constructor keys
and view congruence preserve identity across overlapping Cell instances.
"""
from .terms import actions, text, walk
from . import takeover


def consumer_operators(rules, signatures):
    result=set()
    for rule in rules:
        for fact in rule['body']:
            roots=fact['args'] if fact['kind']=='eq' else [fact['expr']]
            for root in roots:
                for expr in walk(root):
                    head=expr.get('head','')
                    if head.startswith('Spine-') and head[6:] in signatures:
                        result.add(head[6:])
    return result


def rhs_operators(rules, signatures):
    """RHS value requests are readers too, including inherited projections."""
    result=set()
    for rule in rules:
        for action in rule['head']:
            for root in ([action['expr']] if action['kind'] in ('expr','let') else action['args']):
                for expr in walk(root):
                    head=expr.get('head','')
                    if head.startswith('Spine-') and head[6:] in signatures:
                        result.add(head[6:])
    return result


def view_name(op): return 'Spine-Read-'+op


RUNTIME_SCHEDULE='(run-schedule (saturate (seq (saturate (run Spine-Run)) (saturate (run Spine-Reuse)) (run Spine-Create))))'


def definitions(allowed, signatures):
    eqsorts={s['output'] for s in signatures.values()}
    result=['(ruleset Spine-Reuse)','(ruleset Spine-Create)']
    for i,op in enumerate(sorted(allowed)):
        sig=signatures[op];types=[*sig['input'],sig['output']]
        types=['Spine-'+s if s in eqsorts else s for s in types]
        xs=' '.join(f'x{j}' for j in range(len(sig['input'])))
        wanted='Spine-Wanted-'+op
        result += [f'(relation {view_name(op)} ({" ".join(types)}))',
            f'(relation {wanted} ({" ".join(types[:-1])}))',
            f'(rule ((= result (Spine-{op} {xs}))) (({view_name(op)} {xs} result)) :ruleset Spine-Run :name "frontier_seed_{i}")',
            f'(rule (({view_name(op)} {xs} a) ({view_name(op)} {xs} b) (!= a b)) ((union a b)) :ruleset Spine-Run :name "frontier_congruence_{i}")',
            f'(rule (({wanted} {xs}) ({view_name(op)} {xs} result)) ((subsume ({wanted} {xs}))) :ruleset Spine-Reuse :name "frontier_reuse_{i}")',
            f'(rule (({wanted} {xs})) (({view_name(op)} {xs} (Spine-{op} {xs})) (subsume ({wanted} {xs}))) :ruleset Spine-Create :name "frontier_create_{i}")']
    return result


def rule(candidate, allowed, resolve, parse_param, summary):
    state=candidate['snapshot']
    values={i:v['literal'] for i,v in enumerate(state['values']) if v['literal'] is not None}
    equalities=[]
    for raw,name in candidate['params'].items():
        value=resolve(state,parse_param(candidate,raw))
        if value in values and values[value]!=name:
            equalities.append(f'(union {values[value]} {name})')
        else: values[value]=name
    roots=[f'frontier_root_{i}' for i in range(len(candidate['roots']))]
    for expr,name in zip(candidate['roots'],roots):
        value=resolve(state,expr)
        if value in values and values[value]!=name:
            equalities.append(f'(union {values[value]} {name})')
        else: values[value]=name
    initial=set(values)
    rows=[r for r in state['rows'] if r['op'] not in candidate['markers']]
    selected=[(i,r) for i,r in enumerate(state['rows']) if r['op'] in allowed and r['op'] not in candidate['markers']]
    if not selected: return None
    # Anchored spanning constructor witnesses. Keep this finite even if the
    # closed graph has cycles. Roots and RipenInputs are already anchors.
    witnesses={};order=[]
    pending=list(rows)
    while pending:
        deferred=[];changed=False
        for row in pending:
            if row['result'] in values: continue
            if not all(a in values for a in row['args']):
                deferred.append(row);continue
            v=row['result'];values[v]=f'frontier_value_{v}'
            witnesses[v]=row;order.append(v);changed=True
        if not changed: break
        pending=deferred
    needed={v for _,r in selected for v in [*r['args'],r['result']]}
    todo=list(needed)
    while todo:
        v=todo.pop()
        if v in initial:continue
        if v not in witnesses:
            raise ValueError(f'Cell {candidate["family"]}: no certified anchored witness for frontier value {v}')
        for a in witnesses[v]['args']:
            if a not in needed:needed.add(a);todo.append(a)
    # Factor equal dependency sets. Requests are satisfied from known views
    # before the separate creation phase is allowed to construct a witness.
    groups={}
    def dependencies(vertices):
        deps=set();todo=list(vertices)
        while todo:
            v=todo.pop()
            if v in initial or v in deps:continue
            deps.add(v);todo.extend(witnesses[v]['args'])
        return tuple(v for v in order if v in deps)
    def add(deps, head, evidence):
        group=groups.setdefault(deps,dict(head=[],rows=[]))
        group['head'].extend(head);group['rows'].extend(evidence)
    if equalities:add((),equalities,[])
    for v in order:
        if v not in needed:continue
        row=witnesses[v]
        add(dependencies(row['args']),[f'(Spine-Wanted-{row["op"]} {" ".join(values[a] for a in row["args"])})'],[])
    for i,row in selected:
        args=[values[a] for a in row['args']]+[values[row['result']]]
        add(dependencies([*row['args'],row['result']]),[f'({view_name(row["op"])} {" ".join(args)})'],[i])
    result=[]
    for i,(deps,group) in enumerate(groups.items()):
        body=[summary(candidate,candidate['params'],roots)]
        for v in deps:
            w=witnesses[v]
            args=[values[a] for a in w['args']]+[values[v]]
            body.append(f'({view_name(w["op"])} {" ".join(args)})')
        name=f'frontier_{candidate["family"]}_{i}'
        result.append(dict(id=name,target_family=candidate['family'],closure_rows=group['rows'],
            witness_classes=list(deps),witness_ops=sorted({witnesses[v]['op'] for v in needed-initial}),
            rule=f'(rule ({" ".join(body)}) ({" ".join(dict.fromkeys(group["head"]))}) :ruleset Spine-Run :name "{name}")'))
    return result


def inherit(publications, parsed, lenses):
    """Keep views available through larger owners without reviving small Cells."""
    result=[];seen=set()
    for publication in publications:
        source=parsed[publication['id']];old=source['body'][0]['expr']
        choices=[None]+[l for l in lenses if l['child']==publication['target_family']]
        for i,lens in enumerate(choices):
            body=list(source['body'])
            if lens is not None:
                names={name:takeover.var('frontier_owner_'+name) for name in lens['owner_fields']}
                body[0]=dict(kind='fact',expr=takeover.call(f'Spine-Summary-{lens["owner"]}',names.values()))
                body.extend(dict(kind='eq',args=[a,takeover.replace(b,names)]) for a,b in zip(old['args'],lens['args']))
            b,h,key=takeover.normalize(body,source['head'])
            if key in seen:continue
            seen.add(key);name=publication['id']+(f'_cover_{i}' if lens else '')
            result.append(dict(**{k:v for k,v in publication.items() if k not in ('id','rule')},
                id=name,inherited=lens is not None,
                rule=f'(rule ({" ".join(b)}) ({" ".join(h)}) :ruleset Spine-Run :name "{name}")'))
    return result


def rewrite_body(rule, allowed):
    """Replace constructor queries by typed views, preserving all bindings.

    Each nested constructor becomes a view atom plus a shared result variable.
    Equalities, primitive conditions and Summary guards remain unchanged. A
    constructor already matched through a view reuses that exact result handle
    in the RHS; unresolved terms wait for the reuse/creation phases.
    """
    names={e['name'] for f in rule['body'] for root in (f['args'] if f['kind']=='eq' else [f['expr']]) for e in walk(root) if e['kind']=='var'}
    for a in rule['head']:
        if a['kind']=='let':names.add(a['name'])
        for root in ([a['expr']] if a['kind'] in ('expr','let') else a['args']):
            names.update(e['name'] for e in walk(root) if e['kind']=='var')
    extra=[];memo={}
    def fresh():
        name=f'frontier_read_{len(names)}'
        while name in names:name+='x'
        names.add(name);return takeover.var(name)
    def visit(e):
        if e['kind']!='call':return e
        op=e['head'].removeprefix('Spine-')
        if e['head'].startswith('Spine-') and op in allowed:
            key=text(e)
            if key not in memo:
                args=[visit(a) for a in e['args']];v=fresh();memo[key]=v
                extra.append(dict(kind='fact',expr=takeover.call(view_name(op),args+[v])))
            return memo[key]
        return takeover.call(e['head'],[visit(a) for a in e['args']])
    body=[]
    for f in rule['body']:
        if f['kind']=='eq': body.append(dict(kind='eq',args=[visit(a) for a in f['args']]))
        else:
            e=visit(f['expr'])
            if e['kind']=='call':body.append(dict(kind='fact',expr=e))
            # A bare constructor-existence fact is represented by its view atom.
    body=extra+body
    # Constructors in the original RHS are value requests. Match known values
    # first; only still-active requests reach Spine-Create. This is generic
    # positive-constructor evaluation, not a specialized algebraic rule.
    prerequisites=[];requests=[];rhs_memo=dict(memo)
    def head_value(e):
        if text(e) in rhs_memo:return rhs_memo[text(e)]
        if e['kind']!='call':return e
        op=e['head'].removeprefix('Spine-')
        args=[head_value(a) for a in e['args']]
        if e['head'].startswith('Spine-') and op in allowed:
            v=fresh();rhs_memo[text(e)]=v
            req=takeover.call('Spine-Wanted-'+op,args)
            b,h,_=takeover.normalize(body+prerequisites,[dict(kind='expr',expr=req)])
            name=f'{rule["name"]}_request_{len(requests)}'
            requests.append(f'(rule ({" ".join(b)}) ({" ".join(h)}) :ruleset {rule["ruleset"]} :name "{name}")')
            prerequisites.append(dict(kind='fact',expr=takeover.call(view_name(op),args+[v])))
            return v
        return takeover.call(e['head'],args)
    head=[];let_bindings={}
    for a in rule['head']:
        if a['kind']=='let':
            let_bindings[a['name']]=head_value(takeover.replace(a['expr'],let_bindings))
        elif a['kind']=='expr':
            e=head_value(takeover.replace(a['expr'],let_bindings))
            if e['kind']=='call':head.append(dict(kind='expr',expr=e))
        else:head.append(dict(a,args=[head_value(takeover.replace(e,let_bindings)) for e in a['args']]))
    if head:
        b,h,_=takeover.normalize(body+prerequisites,head)
        requests.append(f'(rule ({" ".join(b)}) ({" ".join(h)}) :ruleset {rule["ruleset"]} :name "{rule["name"]}")')
    return requests
