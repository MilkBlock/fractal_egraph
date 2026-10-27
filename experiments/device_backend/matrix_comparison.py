"""Certify fixed-carrier min-choice equality closures from native typed rules.

No rule/table names are recognized. Three exhaustive comparison cases must
union an existing cost witness with a binary choice's result. A schema proof
excludes function callbacks or other constructors feeding equality back into
the selected sort. Hidden relevant rows are a runtime fallback condition.
"""
import hashlib,json

def recognize(rs,tables):
    if len(rs['rules'])!=3:raise ValueError('requires three exhaustive comparison cases')
    cases={};shape=None
    for r in rs['rules']:
        atoms=[a for a in r['body'] if 'table' in a['call']]
        guards=[a for a in r['body'] if 'primitive' in a['call']]
        if len(atoms)!=3 or len(guards)>1:raise ValueError('comparison needs two cost witnesses and one choice')
        counts={a['call']['table']:sum(b['call']['table']==a['call']['table'] for b in atoms) for a in atoms}
        costs=[a for a in atoms if counts[a['call']['table']]==2]
        choices=[a for a in atoms if counts[a['call']['table']]==1]
        if len(costs)!=2 or len(choices)!=1:raise ValueError('cost/choice table roles are ambiguous')
        anchor=choices[0];ct=tables[costs[0]['call']['table']];at=tables[anchor['call']['table']]
        if ct['kind']!='constructor' or len(ct['schema'])!=3 or ct['schema'][1]['kind']!='i64' or ct['schema'][2]['kind']!='eq':raise ValueError('cost witness schema')
        sort=ct['schema'][2]['name']
        if at['kind']!='constructor' or len(at['schema'])!=3 or any(s['kind']!='eq' or s['name']!=sort for s in at['schema']):raise ValueError('choice schema')
        if any('var' not in x for a in atoms for x in a['args']):raise ValueError('literal structural restrictions')
        left,right,result=[x['var'] for x in anchor['args']]
        if len({left,right,result})!=3:raise ValueError('constrained choice variables')
        byresult={a['args'][2]['var']:a for a in costs}
        if set(byresult)!={left,right}:raise ValueError('cost witnesses must describe the choice operands')
        l,rw=byresult[left],byresult[right]
        lt,lc=[x['var'] for x in l['args'][:2]];rt,rc=[x['var'] for x in rw['args'][:2]]
        if len({left,right,result,lt,rt})!=5 or lc in {left,right,result,lt,rt} or rc in {left,right,result,lt,rt}:raise ValueError('extra binding constraints')
        if lc==rc:
            if guards:raise ValueError('extra equality guard')
            comparison='eq'
        else:
            if len(guards)!=1:raise ValueError('missing comparison')
            g=guards[0];c=g['call']
            if c['inputs']!=['i64','i64'] or c['output']!='Unit' or c['primitive'] not in ['<','>']:raise ValueError('unsupported comparison guard')
            args=[x.get('var') for x in g['args'][:2]]
            if args==[lc,rc]:comparison='lt' if c['primitive']=='<' else 'gt'
            elif args==[rc,lc]:comparison='gt' if c['primitive']=='<' else 'lt'
            else:raise ValueError('guard does not compare the cost witnesses')
        aliases={}
        def resolve(x):
            if 'var' not in x:raise ValueError('non-witness effect')
            name=x['var']
            while name in aliases:name=aliases[name]
            return name
        effects=[]
        for a in r['head']:
            if a['op']=='copy':aliases[a['dst']]=resolve(a['arg'])
            else:effects.append(a)
        if len(effects)!=1:raise ValueError('comparison must have one equality effect')
        effect=effects[0];winner=None
        if effect['op']=='set' and effect['call'].get('table')==ct['name'] and resolve(effect['value'])==result:
            args=[resolve(x) for x in effect['args']]
            if args==[lt,lc]:winner='left'
            elif args==[rt,rc]:winner='right'
        elif effect['op']=='union':
            args={resolve(x) for x in effect['args']}
            if args=={result,left}:winner='left'
            elif args=={result,right}:winner='right'
        if winner is None:raise ValueError('effect constructs something beyond existing witnesses')
        if comparison in cases:raise ValueError('duplicate comparison branch')
        cases[comparison]=winner
        current=(ct['name'],at['name'],sort)
        if shape is not None and current!=shape:raise ValueError('inconsistent branch schemas')
        shape=current
    if cases.get('lt')!='left' or cases.get('gt')!='right' or cases.get('eq') not in ['left','right']:raise ValueError('branches do not implement total min choice')
    cost,choice,sort=shape
    affected={sort};changed=True
    def touches(s):return s['name'] in affected or ('element' in s and touches(s['element']))
    while changed:
        old=set(affected)
        for t in tables.values():
            if not any(touches(s) for s in t['schema'][:-1]):continue
            if any(s['kind']=='set' and touches(s) for s in t['schema']):raise ValueError('equality flows into a container key')
            if t['kind']!='constructor':raise ValueError('equality can trigger a function merge')
            if t['schema'][-1]['name']==sort and t['name']!=choice:raise ValueError('another constructor can feed equality back into choice')
            affected.add(t['schema'][-1]['name'])
        changed=old!=affected
    return {'cost_table':cost,'choice_table':choice,'sort':sort,'tie':cases['eq'],'affected_sorts':sorted(affected),
            'rules_sha256':hashlib.sha256(json.dumps(rs['rules'],sort_keys=True).encode()).hexdigest()}

def lower(core,enabled):
    programs=[];rejected=[];tables={t['name']:t for t in core['tables']};ids={t['name']:i for i,t in enumerate(core['tables'])}
    if enabled:
        for sid,rs in enumerate(core['rulesets']):
            try:programs.append(dict(set=sid,name=rs['name'],**recognize(rs,tables)))
            except (ValueError,KeyError) as e:rejected.append({'name':rs['name'],'reason':str(e)})
    cases=''.join(f'case {p["set"]}:return uint3({ids[p["cost_table"]]},{ids[p["choice_table"]]},{int(p["tie"]=="right")});' for p in programs)
    helper='uint3 cu_schema(uint set){switch(set){'+cases+'default:return uint3(0xffffffff);}}\n'
    # Four 8-byte signed-order keys -> two sets of eight byte differences.
    # All coordinates and coefficients are exactly representable in FP16.
    weights=[0]*(32*16)
    for j in range(8):
        weights[j*16+j]=-1;weights[(24+j)*16+j]=1  # max(right) - min(left)
        weights[(8+j)*16+8+j]=1;weights[(16+j)*16+8+j]=-1  # max(left) - min(right)
    return helper,{'enabled':enabled,'programs':programs,'rejected':rejected,'weights':weights,
                   'numeric_contract':'signed i64 order via exact eight-byte coordinates and lexicographic difference; no float cost rounding'}
