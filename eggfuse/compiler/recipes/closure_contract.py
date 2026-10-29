"""Typed closure admission shared by analyzer and runtime validation."""
def bind_roles(atom,roles,env):
    if len(atom['args'])!=len(roles):raise ValueError('helper arity')
    for a,role in zip(atom['args'],roles):
        if 'var' not in a or (a['var'] in env and env[a['var']]!=role):raise ValueError('extra helper binding constraint')
        env[a['var']]=role

def helper_contract(rule,phase,tables):
    atoms=[a for a in rule['body'] if 'table' in a['call']];guards=[a for a in rule['body'] if 'primitive' in a['call']]
    env={};roles=[];pot=[a for a in atoms if a['call']['table']=='PotentialExtractedExpr']
    if len(pot)!=1:raise ValueError('one potential row required per helper')
    bind_roles(pot[0],['root','candidate','potential'],env)
    if phase==0:
        if len(atoms)!=1 or guards:raise ValueError('initializer body')
    else:
        funcs=[a for a in atoms if a['call']['table']=='ExtractedExpr'];costs=[a for a in atoms if a['call']['table']=='TCPair']
        if len(guards)!=1 or guards[0]['call']!={'primitive':'<','inputs':['i64','i64'],'output':'Unit'}:raise ValueError('helper guard')
        if phase==1:
            if len(funcs)!=1 or len(costs)!=2 or len(atoms)!=4:raise ValueError('update helper body')
            bind_roles(funcs[0],['root','old'],env)
        elif funcs or len(costs)!=1 or len(atoms)!=2:raise ValueError('panic helper body')
        for a in costs:
            returned=a['args'][-1].get('var');role=env.get(returned)
            if role not in ['candidate','old']:raise ValueError('unrelated cost witness')
            bind_roles(a,[role+'_term',role+'_cost',role],env)
        args=guards[0]['args'];expected=['candidate_cost','old_cost'] if phase==1 else ['candidate_cost',0]
        actual=[env.get(a.get('var')) if 'var' in a else a['literal'] for a in args[:2]]
        if actual!=expected:raise ValueError('changed helper condition')
    def value(a):return a['literal'] if 'literal' in a else env[a['var']]
    made=[];effects=[]
    for a in rule['head']:
        if a['op']=='copy':env[a['dst']]=value(a['arg'])
        elif a['op']=='let' and 'table' in a['call']:
            expr=[a['call']['table'],*[value(x) for x in a['args']]];env[a['dst']]=expr;made.append(expr)
        elif a['op']=='set':effects.append([a['call']['table'],*[value(x) for x in a['args']],value(a['value'])])
        elif a['op']=='panic':effects.append(['panic'])
        else:raise ValueError('extra helper effect')
    if phase==0:
        if len(made)!=2 or len(made[0])!=1 or made[1][:2]!=['TCPair',made[0]] or not isinstance(made[1][-1],int):raise ValueError('initializer construction')
        if effects!=[['ExtractedExpr','root',made[1]]]:raise ValueError('initializer function update')
        init=tables[made[0][0]]
        if init['kind']!='constructor' or len(init['schema'])!=1 or init['schema'][0]['name']!=tables['TCPair']['schema'][0]['name']:raise ValueError('initializer term sort')
    elif phase==1:
        if made!=[['TCPair','candidate_term','candidate_cost']] or effects!=[['ExtractedExpr','root',made[0]]]:raise ValueError('update helper effects')
    elif made or effects!=[['panic']]:raise ValueError('panic helper effects')
    # Assign each original table atom to the complete row witness retained by
    # the context; no independent marginal is ever used to build a binding.
    for a in atoms:
        name=a['call']['table']
        if name=='PotentialExtractedExpr':roles.append(0)
        elif name=='ExtractedExpr':roles.append(1)
        else:
            ret=a['args'][-1]['var'];roles.append(2 if env[ret]=='candidate' else 3)
    return roles
