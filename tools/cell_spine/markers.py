"""Canonical marker layouts and lowering; no materialized closure-read table.

Ingress reads the raw Spine constructors. Transition outputs are Cano
constructors and therefore cannot become new raw ingress. A Summary is the
active marker itself, not an extra provenance record.
"""
from .terms import text, walk
from . import takeover as t

RUNTIME_SCHEDULE='(run-schedule (saturate Spine-Run))'


def canonical(e, signatures):
    if e['kind']!='call':return e
    head=e['head']
    if head.startswith('Spine-') and head[6:] in signatures:head='Spine-Cano-'+head[6:]
    return t.call(head,[canonical(a,signatures) for a in e['args']])


def layouts(c):
    """Every spine node/input is an anchor. No position sampling or depth cap.

    Each anchor supports three payloads: owner roots and anchor alone, also
    carry inputs outside the anchor, or carry all inputs. Erased inputs are
    recovered from the canonical seed forest with its equality guards. These
    are explicit layout forms, not all possible subsets of payload columns.
    """
    params={raw:t.var(name) for raw,name in c['params'].items()}
    roots=[t.fork(e,params) for e in c['roots']]
    root_names=[f'r{i}' for i in range(len(roots))]
    fields=root_names+list(c['params'].values())
    anchors={text(e):e for root in roots for e in walk(root) if e['kind']!='literal'}
    result=[];seen=set()
    for pos,anchor in enumerate(anchors.values()):
        inside={e['name'] for e in walk(anchor) if e['kind']=='var'}
        for payload in ('anchor','outside','all'):
            carried=[p for p in c['params'].values() if payload=='all' or (payload=='outside' and p not in inside)]
            terms=[t.var(r) for r in root_names]
            if anchor in roots:
                anchor_field=root_names[roots.index(anchor)]
            else:
                anchor_field='anchor';terms.append(anchor)
            terms += [t.var(p) for p in carried if t.var(p) not in terms]
            # A parameter anchor already carries that input, even in read mode.
            query=[]
            stored={term['name'] for term in terms if term['kind']=='var'}
            if set(c['params'].values())-stored:
                # Constructor results are not injective. After an external
                # union, one root may have several argument tuples. Check the
                # complete seed forest to retain correlations across roots.
                query=[dict(kind='eq',args=[t.var(name),root]) for name,root in zip(root_names,roots)]
                if anchor_field=='anchor':query.append(dict(kind='eq',args=[t.var(anchor_field),anchor]))
                query += [dict(kind='eq',guard=True,args=[t.fork(e,params) for e in a['args']])
                          for a in c['acts'] if a['kind']=='union']
            key=(tuple(map(text,terms)),tuple(t.fact_text(q) for q in query))
            if key in seen:continue
            seen.add(key)
            queries=len({text(e) for q in query for root in q['args'] for e in walk(root) if e['kind']=='call'})
            result.append(dict(id=f'm{c["id"]}_{len(result)}',path=c['id'],family=c['family'],position=pos,
                anchor=anchor,anchor_field=anchor_field,fields=fields,terms=terms,decode=query,
                roots=roots,
                columns=len(terms),lookup_atoms=queries,
                payload=payload,scope='owner roots and anchor; optional outside/all inputs; seed-forest decoding preserves correlations'))
    return result


def signature(layout,c):
    fields={f'r{i}':c['signatures'][e['head']]['output'] for i,e in enumerate(c['roots'])}
    fields.update({name:c['param_sorts'][raw] for raw,name in c['params'].items()})
    def sort(e):return fields[e['name']] if e['kind']=='var' else c['signatures'][e['head'][6:]]['output']
    return [sort(e) for e in layout['terms']]


def lower(rule, selected, signatures, *, export=False):
    """Replace full Summary arguments by the selected active marker layout.

    LHS raw facts remain raw. Marker decoding uses only canonical constructors.
    RHS known source terms reuse their matched result, rather than reconstructing
    noncanonical shapes. The canonical target spine is explicitly constructed.
    """
    body=[];known={};certified={};decoded=set();names=set()
    for fact in rule['body']:
        for root in fact['args'] if fact['kind']=='eq' else [fact['expr']]:
            names.update(e['name'] for e in walk(root) if e['kind']=='var')
    for action in rule['head']:
        for root in [action['expr']] if action['kind'] in ('expr','let') else action['args']:
            names.update(e['name'] for e in walk(root) if e['kind']=='var')
    def fresh():
        name=f'marker_value_{len(names)}'
        while name in names:name+='x'
        names.add(name);return t.var(name)
    def bind(expr,value,owned=False):
        key=text(expr)
        if key in known and known[key]!=value:
            body.append(dict(kind='eq',args=[known[key],value]))
        else:known[key]=value
        if owned:certified[key]=known[key]
        return known[key]
    def decode(expr,output=None):
        if expr['kind']!='call':
            if output is not None:body.append(dict(kind='eq',args=[output,expr]))
            return expr
        output=output or known.get(text(expr)) or fresh()
        if text(expr) in decoded:return bind(expr,output,True)
        decoded.add(text(expr))
        args=[decode(a) if a['kind']=='call' else a for a in expr['args']]
        body.append(dict(kind='eq',args=[output,canonical(t.call(expr['head'],args),signatures)]))
        return bind(expr,output,True)
    def flatten(expr):
        if text(expr) in known:return known[text(expr)]
        if expr['kind']!='call':return expr
        if expr['head'].startswith('Spine-') and expr['head'][6:] in signatures:
            args=[flatten(a) for a in expr['args']];v=fresh()
            body.append(dict(kind='eq',args=[v,t.call(expr['head'],args)]))
            return bind(expr,v)
        return t.call(expr['head'],[flatten(a) for a in expr['args']])
    def layout_args(expr):
        layout=selected[int(expr['head'].removeprefix('Spine-Summary-'))]
        subst=dict(zip(layout['fields'],expr['args']))
        return layout,subst,[t.replace(term,subst) for term in layout['terms']]
    # Establish active ownership before resolving other atoms. Each decoded
    # subtree supplies a real canonical value to both LHS and RHS. No lookup
    # relation is emitted, and a retired marker cannot supply these values.
    constraints=[];decoder_indices=set();owners=[]
    for fact in rule['body']:
        if fact['kind']!='fact' or not fact['expr']['head'].startswith('Spine-Summary-'):continue
        expr=fact['expr'];layout,original,_=layout_args(expr)
        # A compound caller binding is still one opaque slot value. Do not
        # accidentally unfold that value as a canonical subtree of this Cell.
        subst={}
        for field,arg in original.items():
            if arg['kind']=='call':
                slot=fresh();constraints.append((slot,arg));subst[field]=slot
            else:subst[field]=arg
        values=[t.replace(term,subst) for term in layout['terms']]
        columns=[]
        for v in values:
            if v['kind']=='call':
                field=fresh();constraints.append((field,v));columns.append(field)
            else:columns.append(v)
        owner=dict(kind='fact',expr=t.call(expr['head'],columns));owners.append(owner)
        body.append(owner)
        for term,column in zip(layout['terms'],columns):
            if term['kind']=='var':subst[term['name']]=column
        for i,root in enumerate(layout['roots']):bind(t.replace(root,subst),columns[i],True)
        if layout['anchor_field']=='anchor':
            subst['anchor']=columns[len(layout['roots'])]
            bind(t.replace(layout['anchor'],subst),subst['anchor'],True)
        decoder_start=len(body)
        for query in layout['decode']:
            output,pattern=[t.replace(x,subst) for x in query['args']]
            if query.get('guard'):
                body.append(dict(kind='eq',args=[flatten(output),flatten(pattern)]))
            else:decode(pattern,output)
        decoder_indices.update(range(decoder_start,len(body)))
    for left,right in constraints:
        body.append(dict(kind='eq',args=[left,flatten(right)]))
    for fact in rule['body']:
        if fact['kind']=='fact' and fact['expr']['head'].startswith('Spine-Summary-'):continue
        if fact['kind']=='eq':body.append(dict(kind='eq',args=[flatten(x) for x in fact['args']]))
        else:body.append(dict(kind='fact',expr=flatten(fact['expr'])))
    def build(expr):
        if text(expr) in certified:return certified[text(expr)]
        if expr['kind']!='call':return expr
        return canonical(t.call(expr['head'],[build(a) for a in expr['args']]),signatures)
    def value(expr):
        if text(expr) in known:return known[text(expr)]
        if expr['kind']!='call':return expr
        return canonical(t.call(expr['head'],[value(a) for a in expr['args']]),signatures)
    head=[]
    def materialize(expr):
        result=build(expr)
        if result['kind']=='call':head.append(dict(kind='expr',expr=result))
        for sub in walk(expr):
            if text(sub) in known:
                head.append(dict(kind='union',args=[known[text(sub)],build(sub)]))
        return result
    for action in rule['head']:
        if export:
            head.append(action);continue
        if action['kind']=='expr' and action['expr']['head'].startswith('Spine-Summary-'):
            expr=action['expr'];layout,subst,args=layout_args(expr)
            for i,root in enumerate(layout['roots']):
                result=materialize(t.replace(root,subst))
                head.append(dict(kind='union',args=[value(expr['args'][i]),result]))
            head.append(dict(kind='expr',expr=t.call(expr['head'],[value(x) for x in args])))
        elif action['kind']=='expr':materialize(action['expr'])
        elif action['kind']=='change' and action['head'].startswith('Spine-Summary-'):
            _,_,args=layout_args(t.call(action['head'],action['args']))
            head.append(dict(action,args=[value(x) for x in args]))
        elif action['kind']=='union':head.append(dict(action,args=[value(e) for e in action['args']]))
        else:head.append(action)
    head=[a for a in head if not (a['kind']=='union' and a['args'][0]==a['args'][1])]
    # An owner certifies that its seed already exists. If this transition uses
    # only stored fields, decoding the rest cannot affect its actions. Keep all
    # decoders for multi-owner joins: erased shared variables still constrain
    # which instances may join, even when the RHS does not use those variables.
    def variables(roots):return {e['name'] for root in roots for e in walk(root) if e['kind']=='var'}
    def fact_roots(f):return f['args'] if f['kind']=='eq' else [f['expr']]
    if len(owners)==1 and decoder_indices:
        introduced=variables(e for i in decoder_indices for e in fact_roots(body[i]))-variables(fact_roots(owners[0]))
        other=[e for i,f in enumerate(body) if i not in decoder_indices for e in fact_roots(f)]
        for a in head:
            other += [a['expr']] if a['kind'] in ('expr','let') else a['args']
            if a['kind']=='set':other.append(a['value'])
        if introduced.isdisjoint(variables(other)):
            body=[f for i,f in enumerate(body) if i not in decoder_indices]
    b,h,_=t.normalize(body,head)
    return f'(rule ({" ".join(b)}) ({" ".join(h)}) :ruleset {rule["ruleset"]} :name "{rule["name"]}")'


def definitions(signatures, fork_lines):
    eqsorts={s['output'] for s in signatures.values()}
    result=[]
    for op,sig in signatures.items():
        inputs=' '.join('Spine-'+s if s in eqsorts else s for s in sig['input'])
        result.append(f'(constructor Spine-Cano-{op} ({inputs}) Spine-{sig["output"]})')
    # Reify both raw ingress facts and the chosen canonical constructors only
    # during explicit export. Original inference rules never run here.
    for line in fork_lines:
        if not line.startswith('(rule '):continue
        for op in signatures:line=line.replace(f'(Spine-{op} ',f'(Spine-Cano-{op} ')
        result.append(line)
    return result
