"""Typed LHS coverage and one-rule alternative entries into known closed Cells.

An indexed match in D supplies a grounded LHS L. Replace seed facts K supplied
by that rule's positive RHS with L. E = L + remaining K is inside D and its
one-step consequence supplies K, hence Sat(E)=D. This is a new entry contract,
not an observed history and not a new closure family. No new seed is recursively
fed back into this pass. Native differential validation is the publication gate.
"""
from collections import defaultdict
from .terms import actions, expressions, text, walk, key
from . import takeover as t
from .composition import positive_environment


def unique(matches):
    seen=set()
    for binding,rows in matches:
        k=tuple(sorted(binding.items())),tuple(sorted(rows))
        if k not in seen:seen.add(k);yield binding,rows


class Facts:
    def __init__(self,state):
        self.state=state;self.by_op=defaultdict(list);self.literals={};self.probes=0
        for i,v in enumerate(state['values']):
            if v.get('literal') is not None:self.literals[v['sort'],v['literal']]=i
        for i,r in enumerate(state['rows']):
            if i not in state.get('subsumed_rows',[]):self.by_op[r['op']].append((i,r))

    def expr(self,e,binding,expected=None):
        kind=e['kind']
        if kind=='var':
            v=binding.get(e['name'],expected)
            if v is not None and (expected is None or v==expected):
                yield dict(binding,**{e['name']:v}),set(),v
        elif kind=='literal':
            v=self.literals.get((e['sort'],e['text']))
            if v is not None and (expected is None or v==expected):yield binding,set(),v
        elif kind=='call':
            for i,r in self.by_op[e['head']]:
                self.probes+=1
                if len(e['args'])!=len(r['args']) or (expected is not None and r['result']!=expected):continue
                partial=[(binding,{i})]
                for arg,v in zip(e['args'],r['args']):
                    partial=list(unique((b,rs|more) for old,rs in partial for b,more,_ in self.expr(arg,old,v)))
                for b,rs in partial:yield b,rs,r['result']

    def fact(self,f,binding):
        if f['kind']=='fact':
            yield from unique((b,rs) for b,rs,_ in self.expr(f['expr'],binding));return
        a,b=f['args']
        if a['kind']=='var' and a['name'] not in binding:a,b=b,a
        yield from unique((end,rs|more) for mid,rs,v in self.expr(a,binding) for end,more,_ in self.expr(b,mid,v))

    def full(self,body,binding=None):
        partial=[(binding or {},set())]
        for f in body:
            partial=list(unique((b,rs|more) for old,rs in partial for b,more in self.fact(f,old)))
            if not partial:break
        return partial


def project(c):
    """One finite ground spelling per value; shortest existing constructor term.

    This only expresses witnesses. It does not identify distinct input ports or
    claim to enumerate all equivalent spellings of a cyclic eclass.
    """
    from .compiler import parse_param,resolve
    state=c['snapshot'];result={};cost={}
    for raw in c['params']:
        e=parse_param(c,raw);v=resolve(state,e)
        if v in result:return None # aliased opaque ports require an explicit interface transport
        result[v]=e;cost[v]=(0,text(e))
    for i,v in enumerate(state['values']):
        if v.get('literal') is not None:
            e=dict(kind='literal',sort=v['sort'],text=v['literal']);result[i]=e;cost[i]=(0,text(e))
    changed=True
    while changed:
        changed=False
        for r in state['rows']:
            if r['op'] in c['markers'] or any(a not in result for a in r['args']):continue
            e=t.call(r['op'],[result[a] for a in r['args']]);rank=(1+sum(cost[a][0] for a in r['args']),text(e))
            if r['result'] not in cost or rank<cost[r['result']]:
                cost[r['result']]=rank;result[r['result']]=e;changed=True
    return result


def calls(acts,params):
    return {text(e) for root in expressions(acts) for e in walk(root) if e['kind']=='call' and text(e) not in params}


def local_binding(body,values,spelling):
    binding={name:spelling[v] for name,v in values.items() if spelling and v in spelling}
    # Native rewrite desugaring names the LHS result. Use that very LHS term
    # for the name, not an arbitrary equivalent expression from the closure.
    # Multiple definitions remain explicit equality guards. Cyclic definitions
    # keep their finite projected witnesses; never unroll an eclass cycle.
    pending={}
    for f in body:
        if f['kind']!='eq':continue
        for lhs,rhs in (f['args'],list(reversed(f['args']))):
            if lhs['kind']=='var' and rhs['kind'] in ('call','literal'):
                pending.setdefault(lhs['name'],rhs)
    while pending:
        ready=[name for name,e in pending.items() if not ({v['name'] for v in walk(e) if v['kind']=='var'} & pending.keys())]
        if not ready:break
        for name in ready:binding[name]=t.replace(pending.pop(name),binding)
    return binding


def seed(body,binding):
    result=[]
    for f in body:
        exprs=[t.replace(e,binding) for e in (f['args'] if f['kind']=='eq' else [f['expr']])]
        if any(e['kind']=='var' for root in exprs for e in walk(root)):raise ValueError('unbound LHS variable')
        for e in exprs:
            if e['kind']=='call':result.append(dict(kind='expr',expr=e))
        if f['kind']=='eq' and exprs[0]!=exprs[1]:result.append(dict(kind='union',args=exprs))
    return list({key(a):a for a in result}.values())


def ground_head(head,binding):
    binding=dict(binding);result=[]
    for a in head:
        if a['kind']=='let':
            value=t.replace(a['expr'],binding);binding[a['name']]=value;result.append(dict(kind='expr',expr=value))
        else:result.append(t.remap_action(a,binding))
    if any(e['kind']=='var' for root in expressions(result) for e in walk(root)):raise ValueError('unbound RHS variable')
    return result


class Index:
    def __init__(self,data):
        self.rules=[];self.by_op=defaultdict(set);self.signatures=data['signatures'];self.positive=positive_environment(data)
        for r in data['rules']:
            body,head,_=t.normalize_ast(r['body'],r['head']);rule=dict(r,body=body,head=head)
            i=len(self.rules);self.rules.append(rule)
            for j,f in enumerate(body):
                for root in f['args'] if f['kind']=='eq' else [f['expr']]:
                    for e in walk(root):
                        if e['kind']=='call':self.by_op[e['head']].add((i,j))

    def inspect(self,c):
        from .compiler import resolve
        facts=Facts(c['snapshot']);spelling=project(c);records=[];entries=[]
        user_rows={i for i,r in enumerate(c['snapshot']['rows']) if r['op'] not in c['markers']}
        row_ids={(r['op'],tuple(r['args']),r['result']):i for i,r in enumerate(c['snapshot']['rows'])}
        seed_rows={row_ids[e['head'],tuple(resolve(c['snapshot'],a) for a in e['args']),resolve(c['snapshot'],e)]
                   for root in expressions(c['acts']) for e in walk(root) if e['kind']=='call' and e['head'] not in c['markers']}
        ids=sorted({i for op in facts.by_op if op not in c['markers'] for i,_ in self.by_op[op]})
        supported=(self.positive and c.get('boundary_positive') and spelling is not None
            and c['snapshot'].get('scope',{}).get('semantics')=='positive-constructor-equality/v1'
            and not c['snapshot'].get('ports') and not c['snapshot'].get('subsumed_rows'))
        for i in ids:
            rule=self.rules[i];body=rule['body']
            if not body:continue
            full=list(facts.full(body));full_keys={tuple(sorted(b.items())) for b,_ in full}
            witnesses=[(b,rows,list(range(len(body))),[]) for b,rows in full]
            # Partial records are exact anchored witnesses, not a powerset of
            # LHS subsets. Unbound conditions remain explicitly unresolved.
            anchors={tuple(sorted(b.items())):b for f in body for b,_ in facts.fact(f,{})}
            for k,b in anchors.items():
                if k in full_keys:continue
                covered=[];missing=[];rows=set()
                for j,f in enumerate(body):
                    variables={e['name'] for root in (f['args'] if f['kind']=='eq' else [f['expr']]) for e in walk(root) if e['kind']=='var'}
                    matches=list(facts.fact(f,b)) if variables<=b.keys() else []
                    if matches:covered.append(j);rows.update(set().union(*(rs for _,rs in matches)))
                    else:missing.append(j)
                if covered:witnesses.append((b,rows,covered,missing))
            for b,rows,covered,missing in witnesses:
                projected=local_binding(body,b,spelling)
                record=dict(cell_unit=c['id'],family=c['family'],rule=rule['name'],rule_id=i,
                    binding={name:dict(value=v,sort=c['snapshot']['values'][v]['sort'],term=text(projected[name]) if name in projected else None) for name,v in b.items()},
                    covered_lhs_positions=covered,covered_cell_rows=sorted(rows),
                    lhs_complete=not missing,lhs_covers_cell_rows=user_rows<=rows,lhs_covers_seed_rows=seed_rows<=rows,
                    seed_equality_obligations=actions([a for a in c['acts'] if a['kind']=='union']),
                    remaining_lhs=[t.fact_text(t.remap_fact(body[j],projected)) for j in missing],
                    proof_status='partial' if missing else 'matched-in-closed-cell',
                    scope='partial rows are anchored witnesses; not all maximal partial joins')
                records.append(record)
                if missing or not supported:continue
                try:
                    lhs=seed(body,projected);head=ground_head(rule['head'],projected)
                    provided=calls(head,c['params'])
                    remaining=[a for a in c['acts'] if a['kind']!='expr' or text(a['expr']) not in provided]
                    removed=[a for a in c['acts'] if a not in remaining]
                    if not removed:record['entry_reason']='RHS supplies no complete seed fact';continue
                    entry=list({key(a):a for a in lhs+remaining if not(a['kind']=='expr' and text(a['expr']) in c['params'])}.values())
                    available={text(e) for root in expressions(entry) for e in walk(root)}
                    if not set(c['params'])<=available:record['entry_reason']='entry would lose an input port';continue
                    if set(map(key,c['acts']))<=set(map(key,entry)):record['entry_reason']='existing seed already directly available';continue
                    # Check every new assertion and equality against D. Do not
                    # use equality in D to silently erase an entry guard.
                    for a in entry+head:
                        values=[resolve(c['snapshot'],e) for e in expressions([a])]
                        if a['kind']=='union' and len(set(values))!=1:raise ValueError('equality not in target')
                    record.update(proof_status='one-rule-entry-certified',removed_seed_facts=actions(removed),
                        entry_facts=actions(entry),proof='E = instantiated LHS + unsupplied seed; E subset D; rule RHS supplies missing K; Sat(K)=D implies Sat(E)=D')
                    entries.append((c,entry,record))
                except (KeyError,StopIteration,ValueError) as e:record['entry_reason']=str(e)
        return records,entries,facts.probes


def entry_key(acts,params):
    terms=[]
    for a in acts:
        if a['kind']=='union':terms.append(('eq',tuple(sorted(text(e,params=params) for e in a['args']))))
        else:terms.append(('fact',text(a['expr'],params=params)))
    return tuple(sorted(set(terms)))


def footprint(c,acts,params):
    nodes={text(e,params=params) for root in expressions(acts) for e in walk(root) if e['kind']=='call' and text(e) not in c['params']}
    equalities={tuple(sorted(text(e,params=params) for e in a['args'])) for a in acts if a['kind']=='union'}
    return nodes,{p for p in equalities if p[0]!=p[1]}


def extend(data,candidates,representations,edges,*,emit=True):
    from .compiler import lower_option,mapping_to
    index=Index(data);groups=defaultdict(list)
    for c in representations:groups[c['family']].append(c)
    records=[];entries=[];seen=set();probes=0;known=defaultdict(list)
    # Existing history recipes with the same exact entry need no new rule.
    for c in candidates:
        normalized=mapping_to(c,groups[c['family']][0])
        seen.add((c['family'],entry_key(c['acts'],normalized)))
        known[c['family']].append(footprint(c,c['acts'],normalized))
    for c in representations:
        found,derived,count=index.inspect(c);records.extend(found);probes+=count
        for base,acts,proof in derived:
            identity=(c['family'],entry_key(acts,mapping_to(c,groups[c['family']][0])))
            if identity in seen:proof['entry_reason']='duplicate known entry';continue
            footprint_=footprint(c,acts,mapping_to(c,groups[c['family']][0]))
            if any(ns<=footprint_[0] and eqs<=footprint_[1] for ns,eqs in known[c['family']]):
                proof['entry_reason']='a known family seed already covered by this entry';continue
            seen.add(identity);number=len(entries)
            entry=dict(base,acts=acts);options=[]
            for target in groups[c['family']]:
                opt=lower_option(entry,target,[],{})
                opt.update(id=f'lhs_entry_{number}_{len(options)}',proof_kind='certified_lhs_entry');options.append(opt)
            proof['entry_id']=f'lhs_entry_{number}'
            proof['emitted']=emit
            desc=dict(id=f'lhs_entry_{number}',kind='certified_lhs_entry',target_unit=c['id'],observed_parents=[],
                      entry_facts=actions(acts),entry_actions=acts,proof=proof,options=options)
            entries.append(desc)
            if emit:edges.append(desc)
    return dict(schema='cell-lhs-coverage/v1',positive_environment=index.positive,
        fragment='completed Cell closure; opaque parameter constructors excluded from coverage counts',
        counts_scope='per distinct seed representation; one closure family can have several seed representations',
        rule_count=len(index.rules),posting_count=sum(map(len,index.by_op.values())),snapshot_row_visits=probes,
        rules=[dict(id=i,name=r['name'],lhs=list(map(t.fact_text,r['body']))) for i,r in enumerate(index.rules)],
        index=[dict(operator=op,signature=index.signatures.get(op),positions=[dict(rule_id=i,lhs_position=j) for i,j in sorted(ps)]) for op,ps in sorted(index.by_op.items())],
        full_matches=sum(r['lhs_complete'] for r in records),partial_witnesses=sum(not r['lhs_complete'] for r in records),
        matching_rules=len({r['rule_id'] for r in records}),new_entries=len(entries),emitted_entries=len(entries) if emit else 0,
        entries=entries,records=records,
        scope='one-rule predecessor entries for known finite positive closures; no recursive seed search, new closure families, primitive guard evaluation or automatic port-alias transport')
