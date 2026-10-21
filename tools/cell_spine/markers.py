"""Canonical marker layouts and physical-storage lowering.

Ingress reads raw Spine constructors. Storage contracts either borrow these
immutable rows or construct Cano rows; neither publishes new raw ingress. A Summary is the
active marker itself, not an extra provenance record.
"""
from .terms import actions, text, walk
from . import takeover as t
from . import names as naming
import json

RUNTIME_SCHEDULE='(run-schedule (saturate Run!))'


def layout_roles(rule):
    """The only three sites where lowering reads a marker layout."""
    roles={}
    def add(expr,role):
        roles.setdefault(naming.family(expr['head']),set()).add(role)
    for f in rule['body']:
        if f['kind']=='fact' and naming.is_summary(f['expr']['head']):add(f['expr'],'read')
    for a in rule['head']:
        if a['kind']=='expr' and naming.is_summary(a['expr']['head']):add(a['expr'],'create')
        elif a['kind']=='change' and naming.is_summary(a['head']):add(a,'retire')
    return roles


def layout_projection(layout,roles):
    """Exact transition-specific recipe, not observed cost equivalence.

    Reflect _lower_ast's field accesses for read/create/retire independently.
    Keep binding expressions, guards and physical storage. Global marker-column
    storage remains priced on the original variables, outside this projection.
    """
    if not roles:return 'unused'
    result=dict(fields=layout['fields'],terms=layout['terms'])
    canonical=layout.get('representation','canonical')=='canonical'
    if 'read' in roles:
        result['read']=dict(roots=layout['roots'],decode=layout['decode'],
            interface=layout.get('interface','inline'),
            canonical=canonical,storage=layout.get('read_storage','canonical'),
            anchor=layout['anchor'] if layout['anchor_field']=='anchor' else None)
    if 'create' in roles:
        result['create']=dict(roots=layout['roots'],canonical=canonical,
            stored=None if canonical else layout['stored_roots'],
            borrow=layout.get('representation') in ('borrowed','boundary_borrowed'))
    return json.dumps(result,sort_keys=True)


def canonical(e, signatures):
    if e['kind']!='call':return e
    head=e['head']
    if naming.is_raw(head,signatures):head=naming.canonical(head[:-1])
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
    def sort(e):return fields[e['name']] if e['kind']=='var' else c['signatures'][e['head'][:-1]]['output']
    return [sort(e) for e in layout['terms']]


def normalize_lookups(body, head, signatures, cache=None):
    """Cache exact LHS aliases/existence queries, then substitute the new RHS.

    The final decoder decision and target-induced lookups are already in body.
    Consequently this key cannot merge guarded, raw/canonical or alias variants.
    """
    key=json.dumps(body,sort_keys=True) if cache is not None else None
    plan=cache.get(key) if cache is not None else None
    if plan is None:
        constructors={name for op,sig in signatures.items() if sig.get('kind','constructor')=='constructor'
                      for name in (naming.raw(op),naming.canonical(op))}
        body=[dict(f,args=list(reversed(f['args']))) if f['kind']=='eq' and
              f['args'][0]['kind']=='call' and f['args'][1]['kind']=='var' and
              f['args'][0]['head'] in constructors else f for f in body]
        subst={}
        while True:
            step=t.variable_aliases(body)
            subst={k:t.replace(v,step) for k,v in subst.items()};subst.update(step)
            body=[t.remap_fact(f,step) for f in body]
            body=[f for f in body if not (f['kind']=='eq' and f['args'][0]==f['args'][1] and f['args'][0]['kind']=='var')]
            body=list({t.fact_text(f):f for f in body}.values())
            outputs={};aliases=[]
            for fact in body:
                if fact['kind']!='eq':continue
                left,right=fact['args']
                if left['kind']=='call':left,right=right,left
                if left['kind']!='var' or right['kind']!='call' or right['head'] not in constructors:continue
                row=text(right)
                if row in outputs and outputs[row]!=left:
                    aliases.append(dict(kind='eq',args=[outputs[row],left]))
                else:outputs[row]=left
            if not aliases:break
            body=body+aliases
        plan=(body,subst)
        if cache is not None:cache[key]=plan
    body,subst=plan
    head=[t.remap_action(a,subst) for a in head]
    head=list({actions([a])[0]:a for a in head}.values())
    head=[a for a in head if not (a['kind']=='union' and a['args'][0]['kind']=='var'
                                  and a['args'][0]==a['args'][1])]
    return list(body),head


def lower_ast(rule, selected, signatures, *, export=False):
    """Lower a whole rule; also the uncached oracle for pricing ablations."""
    return _lower_ast(rule, selected, signatures, export=export)


def prepare_source(rule, selected, signatures):
    """Compile owners/LHS once, retaining undecided decoder obligations.

    The returned ASTs are immutable by convention. Resuming copies every mutable
    collection, so one target cannot alter another target's source certificate.
    """
    return _lower_ast(rule, selected, signatures, stop_after_source=True)


def resume_source(rule, selected, signatures, source, *, normalizations=None):
    return _lower_ast(rule, selected, signatures, source=source, normalizations=normalizations)


def _lower_ast(rule, selected, signatures, *, export=False, source=None, stop_after_source=False, normalizations=None):
    """Logical value identity and physical canonical rows remain distinct."""
    if 'interface_family' in rule:
        from .interfaces import producer
        return {} if stop_after_source else producer(rule,selected,signatures)
    if source is None:
        body=[];known={};certified={};canonical_rows=set();decoded=set();names=set();reused=set()
        for fact in rule['body']:
            for root in fact['args'] if fact['kind']=='eq' else [fact['expr']]:
                names.update(e['name'] for e in walk(root) if e['kind']=='var')
        for action in rule['head']:
            for root in [action['expr']] if action['kind'] in ('expr','let') else action['args']:
                names.update(e['name'] for e in walk(root) if e['kind']=='var')
    else:
        body=list(source['body'])
        known=dict(source['known'])
        certified=dict(source['certified'])
        canonical_rows=set(source['canonical_rows'])
        decoded=set(source['decoded'])
        names=set(source['names'])
        reused=set(source['reused'])
        decoder_indices=set(source['decoder_indices'])
        owners=list(source['owners'])
        owner_interfaces=list(source['owner_interfaces'])
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
    def decode(expr,output=None,storage='canonical'):
        if expr['kind']!='call':
            if output is not None:body.append(dict(kind='eq',args=[output,expr]))
            return expr
        output=output or known.get(text(expr)) or fresh()
        if text(expr) in decoded:return bind(expr,output,True)
        decoded.add(text(expr))
        args=[decode(a,storage=storage) if a['kind']=='call' else a for a in expr['args']]
        row=t.call(expr['head'],args)
        body.append(dict(kind='eq',args=[output,row if storage=='raw' else canonical(row,signatures)]))
        if storage=='canonical':canonical_rows.add(text(expr))
        return bind(expr,output,True)
    def flatten(expr):
        if text(expr) in known:
            if text(expr) in certified:reused.add(text(expr))
            return known[text(expr)]
        if expr['kind']!='call':return expr
        if naming.is_raw(expr['head'],signatures):
            args=[flatten(a) for a in expr['args']];row=t.call(expr['head'],args)
            if text(row) in known:return bind(expr,known[text(row)])
            v=fresh();body.append(dict(kind='eq',args=[v,row]))
            bind(row,v)
            return bind(expr,v)
        return t.call(expr['head'],[flatten(a) for a in expr['args']])
    def layout_args(expr):
        layout=selected[naming.family(expr['head'])]
        subst=dict(zip(layout['fields'],expr['args']))
        return layout,subst,[t.replace(term,subst) for term in layout['terms']]
    if source is None:
        # Establish active ownership before resolving other atoms. Each decoded
        # subtree supplies a real canonical value to both LHS and RHS. No lookup
        # relation is emitted, and a retired marker cannot supply these values.
        constraints=[];decoder_indices=set();owners=[];owner_interfaces=[]
        for fact in rule['body']:
            if fact['kind']!='fact' or not naming.is_summary(fact['expr']['head']):continue
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
            # A published ABI stores intermediate witnesses as columns too.
            # Their rows were materialized by the Summary's creating action;
            # retain these exact correlated values instead of decoding again.
            if layout.get('interface')=='published':
                for term,column in zip(layout['terms'],columns):
                    if term['kind']=='call':
                        concrete=t.replace(term,subst)
                        bind(concrete,column,True);canonical_rows.add(text(concrete))
            for i,root in enumerate(layout['roots']):
                term=t.replace(root,subst);bind(term,columns[i],True)
                if layout.get('representation','canonical')=='canonical':canonical_rows.add(text(term))
            if layout['anchor_field']=='anchor':
                subst['anchor']=columns[len(layout['roots'])]
                term=t.replace(layout['anchor'],subst);bind(term,subst['anchor'],True)
                if layout.get('read_storage','canonical')=='canonical':canonical_rows.add(text(term))
            exposed=[t.replace(r,subst) for r in layout['roots']]
            if layout['anchor_field']=='anchor':exposed.append(t.replace(layout['anchor'],subst))
            owner_interfaces.append(dict(exposed=exposed,roots=[t.replace(r,subst) for r in layout['roots']],
                roots_only=len(layout['terms'])==len(layout['roots']),
                storage=layout.get('read_storage','canonical'),guard=any(q.get('guard') for q in layout['decode'])))
            decoder_start=len(body)
            for query in layout['decode']:
                output,pattern=[t.replace(x,subst) for x in query['args']]
                if query.get('guard'):
                    body.append(dict(kind='eq',args=[flatten(output),flatten(pattern)]))
                else:decode(pattern,output,layout.get('read_storage','canonical'))
            decoder_indices.update(i for i in range(decoder_start,len(body))
                                   if any(e['kind']=='call' for r in body[i]['args'] for e in walk(r)))
            if layout.get('interface')=='shared' and not export:
                from .interfaces import coordinates, name
                args=[]
                for coord in coordinates(layout):
                    e=t.replace(coord,subst)
                    args.append(known[text(e)] if e['kind']=='call' else e)
                # The marker stays in body; a stale cached row alone cannot
                # enable a consumer after the owner's retirement.
                body[decoder_start:]=[dict(kind='fact',expr=t.call(name(naming.family(expr['head'])),args))]
                decoder_indices.difference_update(i for i in list(decoder_indices) if i>=decoder_start)
        for left,right in constraints:
            body.append(dict(kind='eq',args=[left,flatten(right)]))
        for fact in rule['body']:
            if fact['kind']=='fact' and naming.is_summary(fact['expr']['head']):continue
            if fact['kind']=='eq':body.append(dict(kind='eq',args=[flatten(x) for x in fact['args']]))
            else:body.append(dict(kind='fact',expr=flatten(fact['expr'])))
    if stop_after_source:
        return dict(body=body,known=known,certified=certified,canonical_rows=canonical_rows,decoded=decoded,names=names,reused=reused,decoder_indices=decoder_indices,owners=owners,owner_interfaces=owner_interfaces)
    def build(expr):
        if text(expr) in certified and text(expr) in canonical_rows:
            reused.add(text(expr));return certified[text(expr)]
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
        if action['kind']=='expr' and naming.is_summary(action['expr']['head']):
            expr=action['expr'];layout,subst,args=layout_args(expr)
            if layout.get('representation','canonical')=='canonical':
                for i,root in enumerate(layout['roots']):
                    result=materialize(t.replace(root,subst))
                    head.append(dict(kind='union',args=[value(expr['args'][i]),result]))
            else:
                # Roots already have identities from the certified raw input.
                # Store only the chosen physical subtree, or borrow it too.
                for root in layout['stored_roots']:materialize(t.replace(root,subst))
                for original,root in zip(expr['args'],layout['roots']):
                    result=flatten(t.replace(root,subst))
                    head.append(dict(kind='union',args=[flatten(original),result]))
            borrow=layout.get('representation') in ('borrowed','boundary_borrowed')
            head.append(dict(kind='expr',expr=t.call(expr['head'],[(flatten(x) if borrow else value(x)) for x in args])))
        elif action['kind']=='expr':materialize(action['expr'])
        elif action['kind']=='change' and naming.is_summary(action['head']):
            _,_,args=layout_args(t.call(action['head'],action['args']))
            head.append(dict(action,args=[value(x) for x in args]))
        elif action['kind']=='union':head.append(dict(action,args=[value(e) for e in action['args']]))
        else:head.append(action)
    head=[a for a in head if not (a['kind']=='union' and a['args'][0]==a['args'][1])]
    # An owner certifies that its seed already exists. If this transition uses
    # only stored fields, decoding the rest cannot affect its actions. Multi-owner
    # joins additionally require compatible exposed subtree witnesses below.
    def variables(roots):return {e['name'] for root in roots for e in walk(root) if e['kind']=='var'}
    def fact_roots(f):return f['args'] if f['kind']=='eq' else [f['expr']]
    # For several owners, one owner must expose every subordinate forest
    # directly, and subordinates must carry roots only. Arbitrary overlapping
    # hidden variables (or different physical tables) still require a join.
    compatible=not any(i['guard'] for i in owner_interfaces)
    if len(owners)>1:
        compatible=compatible and any(all(j==k or (child['roots_only'] and
            child['storage']==owner['storage'] and all(r in owner['exposed'] for r in child['roots']))
            for k,child in enumerate(owner_interfaces)) for j,owner in enumerate(owner_interfaces))
    if compatible and decoder_indices:
        introduced=variables(e for i in decoder_indices for e in fact_roots(body[i]))-variables(e for owner in owners for e in fact_roots(owner))
        other=[e for i,f in enumerate(body) if i not in decoder_indices for e in fact_roots(f)]
        for a in head:
            other += [a['expr']] if a['kind'] in ('expr','let') else a['args']
            if a['kind']=='set':other.append(a['value'])
        if introduced.isdisjoint(variables(other)):
            body=[f for i,f in enumerate(body) if i not in decoder_indices]
    body,head=normalize_lookups(body,head,signatures,normalizations)
    return dict(name=rule['name'],ruleset=rule['ruleset'],body=body,head=head,reused_subtrees=sorted(reused))


def render(rule):
    from .terms import actions
    if rule.get('disabled'):return ''
    return f'(rule ({" ".join(t.fact_text(f) for f in rule["body"])}) ({" ".join(actions(rule["head"]))}) :ruleset {rule["ruleset"]} :name "{rule["name"]}")'


def lower(rule, selected, signatures, *, export=False):
    return render(lower_ast(rule,selected,signatures,export=export))


def definitions(signatures, fork_lines):
    eqsorts={s['output'] for s in signatures.values()}
    result=[]
    for op,sig in signatures.items():
        inputs=' '.join(naming.raw(s) if s in eqsorts else s for s in sig['input'])
        result.append(f'(constructor {naming.canonical(op)} ({inputs}) {naming.raw(sig["output"])})')
    # Reify both raw ingress facts and the chosen canonical constructors only
    # during explicit export. Original inference rules never run here.
    for line in fork_lines:
        if not line.startswith('(rule '):continue
        for op in signatures:line=line.replace(f'({naming.raw(op)} ',f'({naming.canonical(op)} ')
        result.append(line)
    return result
