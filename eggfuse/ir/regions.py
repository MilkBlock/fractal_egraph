"""Executable Spine-plan validation, independent of analyzer optimization passes."""
from .recipe import digest, require, validate

SCHEMA='eggfuse-regions/v1'


def query_order(body,pivot,marker_names):
    pending=[dict(a,_ordinal=i) for i,a in enumerate(body)]
    known=set();result=[];table_index=0
    for a in pending:
        if 'table' in a['call']:a['_table_atom']=table_index;table_index+=1
    pivot_pending=pivot is not None
    while pending:
        candidates=[i for i,a in enumerate(pending) if 'table' in a['call'] or all('literal' in x or x['var'] in known for x in a['args'][:-1])]
        require(candidates,'ungrounded primitive in region query')
        def rank(i):
            a=pending[i]
            return (3 if 'primitive' in a['call'] else (2 if pivot_pending and a.get('_table_atom')==pivot else 0),
                    int(a['call'].get('table') in marker_names),sum('literal' in x or x.get('var') in known for x in a['args']),-i)
        a=pending.pop(max(candidates,key=rank));result.append(a)
        pivot_pending &= a.get('_table_atom')!=pivot
        known.update(x['var'] for x in a['args'] if 'var' in x)
    return result


def validate_plan(plan,recipe):
    core=validate(recipe)
    require(plan.get('schema')==SCHEMA,'unknown region program')
    require(plan.get('sha256')==digest({k:v for k,v in plan.items() if k!='sha256'}),'region checksum differs')
    require(plan['recipe_sha256']==recipe['sha256'] and plan['scope']==recipe['scope'],'region source/scope differs')
    require(plan['control']==core['schedule'],'region control differs')
    tables={t['name']:t for t in core['tables']};nodes=plan['nodes'];patterns=[]
    require(plan['marker_names']==sorted(set(plan['marker_names'])) and all(t in tables for t in plan['marker_names']),'unknown region marker table')
    for ni,n in enumerate(nodes):
        parent=n['parent'];require(n['id']==ni and (parent is None or type(parent) is int and 0<=parent<ni),'invalid Spine dependency')
        atom=n['atom'];require(atom['table'] in tables,'unknown Spine table')
        signature=tables[atom['table']]['schema'];require(len(atom['args'])==len(signature),'Spine arity differs')
        previous=nodes[parent] if parent is not None else None
        known=set(range(len(previous['sorts']))) if previous else set()
        if previous:
            require(n['sorts'][:len(previous['sorts'])]==previous['sorts'],'parent slot type differs')
            require(all('literal' in x or x.get('slot') in known for x in atom['args'][:-1]),'Spine extension is not fully keyed')
        for x,sort in zip(atom['args'],signature):
            require(('slot' in x)!=('literal' in x),'invalid Spine operand')
            if 'slot' in x:
                i=x['slot'];require(type(i) is int and 0<=i<len(n['sorts']) and n['sorts'][i]==sort,'Spine slot sort differs')
                known.add(i)
            else:require(x['sort']==sort,'Spine literal sort differs')
        require(known==set(range(len(n['sorts']))),'unbound Spine slot')
        chain=(patterns[parent] if parent is not None else [])+[atom];patterns.append(chain)
        require(n['tables']==len(chain) and n['width']==len(n['sorts'])+2*len(chain),'Spine layout differs')
        require(n['reads']==sorted({a['table'] for a in chain}),'Spine subscriptions differ')
    originals=[];keys={};sets=[]
    for si,rs in enumerate(core['rulesets']):
        uses=[]
        for ordinal,r in enumerate(rs['rules']):
            key=(r.get('source_set',rs['name']),r['name'])
            if key not in keys:
                keys[key]=len(originals);originals.append((key,r,recipe['rulesets'][si]['rule_ids'][ordinal]))
            else:require(originals[keys[key]][1]==r,'region rule identity changed')
            uses.append(keys[key])
        sets.append(dict(name=rs['name'],rules=uses))
    require(plan['rulesets']==sets and len(plan['rules'])==len(originals),'region rule coverage differs')
    for rid,(key,rule,recipe_id) in enumerate(originals):
        r=plan['rules'][rid];require(r['id']==rid and r['identity']==list(key) and r['recipe_rule']==recipe_id,'region rule identity differs')
        footprint=recipe['rules'][recipe_id]['footprint']
        require(r['reads']==footprint['read_tables'] and r['writes']==footprint['write_tables'] and r['operations']==footprint['operations'] and r['union_sorts']==footprint['union_sorts'],'region effect footprint differs')
        nt=sum('table' in a['call'] for a in rule['body']);pivots=list(range(nt)) if rule['seminaive'] and nt else [None]
        require([v['pivot'] for v in r['variants']]==pivots,'region delta variants differ')
        for v in r['variants']:
            order=query_order(rule['body'],v['pivot'],plan['marker_names'])
            require(v['order']==[a['_ordinal'] for a in order],'region witness order differs')
            if v['node'] is None:
                require(not v['slots'],'uncached variant declares guaranteed slots');continue
            require(type(v['node']) is int and 0<=v['node']<len(nodes),'invalid consumer Spine')
            prefix=order[:len(v['prefix'])]
            require(v['prefix']==[a['_ordinal'] for a in prefix] and all('table' in a['call'] for a in prefix),'consumer prefix differs')
            require(v['table_atoms']==[a['_table_atom'] for a in prefix],'consumer witness stamps differ')
            names={};sorts=[];expected=[]
            for a in prefix:
                args=[]
                for x in a['args']:
                    if 'literal' in x:args.append(x)
                    else:
                        if x['var'] not in names:names[x['var']]=len(names);sorts.append(x['sort'])
                        args.append({'slot':names[x['var']]})
                expected.append(dict(table=a['call']['table'],args=args))
            require(expected==patterns[v['node']] and sorts==nodes[v['node']]['sorts'] and names==v['slots'],'consumer binding wiring differs')
    members=[rid for region in plan['regions'] for rid in region['rules']]
    require(sorted(members)==list(range(len(originals))),'region partition omits or duplicates rules')
    return plan
