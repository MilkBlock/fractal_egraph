"""Exact affine slice extraction from real Cell rows and parsed source rules.

No fitted coefficients, table-count features, or equality IDs as numeric values.
Unsupported scalar alternatives/guards are rejected, never approximated.
"""
from collections import defaultdict

TRACKED={'Expr-size'}
PRIMITIVES={'+','-','*','min','max','<','>','<=','>=','!='}


def contains(expr,heads):
    return expr.get('head') in heads or any(contains(a,heads) for a in expr.get('args',[]))


def exprs(fact):return fact['args'] if fact['kind']=='eq' else [fact['expr']]


class Graph:
    def __init__(self,snapshot):
        self.data=snapshot;self.rows=snapshot['rows'];self.values=snapshot['values']
        hidden=set(snapshot.get('subsumed_rows',[]))
        self.by_op=defaultdict(list)
        for i,row in enumerate(self.rows):
            if i not in hidden:self.by_op[row['op']].append((i,row))

    def match(self,pattern,value,env):
        kind=pattern['kind']
        if kind=='var':
            name=pattern['name']
            if name in env and env[name]!=value:return []
            return [{**env,name:value}]
        if kind=='literal':
            actual=self.values[value]
            return [env] if (actual['sort'],actual['literal'])==(pattern['sort'],pattern['text']) else []
        results=[]
        for _,row in self.by_op[pattern['head']]:
            if row['result']!=value or len(row['args'])!=len(pattern['args']):continue
            candidates=[env]
            for p,v in zip(pattern['args'],row['args']):
                candidates=[new for old in candidates for new in self.match(p,v,old)]
            results.extend(candidates)
        return results

    def possible(self,expr,env):
        if expr['kind']=='var':
            if expr['name'] not in env:raise ValueError('unbound structural variable')
            return [(env[expr['name']],env)]
        if expr['kind']=='literal':
            return [(i,env) for i,v in enumerate(self.values) if (v['sort'],v['literal'])==(expr['sort'],expr['text'])]
        return [(row['result'],new) for _,row in self.by_op[expr['head']] for new in self.match(expr,row['result'],env)]

    def resolve(self,expr,env=None):
        matches={value for value,_ in self.possible(expr,env or {})}
        if len(matches)!=1:raise ValueError(f'ground expression has {len(matches)} results')
        return next(iter(matches))

    def query(self,facts):
        candidates=[{}]
        for fact in facts:
            next_candidates=[]
            for env in candidates:
                if fact['kind']=='fact':
                    next_candidates += [new for _,new in self.possible(fact['expr'],env)]
                else:
                    a,b=fact['args']
                    if a['kind']=='var' and a['name'] not in env:a,b=b,a
                    for value,new in self.possible(a,env):
                        next_candidates+=self.match(b,value,new)
            # Set semantics for named binding environments.
            candidates=[dict(items) for items in sorted({tuple(sorted(e.items())) for e in next_candidates})]
        return candidates


def scalar(expr,env,locals_,graph):
    kind=expr['kind']
    if kind=='literal':
        if expr['sort']!='i64':raise ValueError('only i64 scalar arithmetic is certified')
        return ('const',expr['value'])
    if kind=='var':
        name=expr['name']
        if name in locals_:return locals_[name]
        v=graph.values[env[name]]
        if v['sort']!='i64' or v['literal'] is None:raise ValueError('non-scalar binding used as number')
        return ('const',int(v['literal']))
    op=expr['head']
    if op in TRACKED:
        if len(expr['args'])!=1:raise ValueError('unsupported field arity')
        return ('field',graph.resolve(expr['args'][0],env))
    if op not in {'+','-','*','min','max'}:raise ValueError(f'unsupported arithmetic: {op}')
    return (op,tuple(scalar(a,env,locals_,graph) for a in expr['args']))


def scalar_subexpressions(expr):
    """Definedness belongs to every primitive, even if normalization cancels it."""
    yield expr
    if expr[0] not in {'const','field'}:
        for child in expr[1]:yield from scalar_subexpressions(child)


def extract_equations(graph,ast,rulesets):
    merge=ast['signatures']['Expr-size'].get('merge')
    assert merge and merge['kind']=='call' and merge['head']=='min'
    assert [a.get('name') for a in merge['args']]==['old','new']
    equations=defaultdict(list);certificates=[];blocked_rules=[]
    active=[r for r in ast['rules'] if r['ruleset'] in rulesets]
    for rule in active:
        heads=rule['head']
        size_only=len(heads)==1 and heads[0]['kind']=='set' and heads[0]['head']=='Expr-size'
        reads_size=any(contains(e,TRACKED) for f in rule['body'] for e in exprs(f))
        if not size_only and not reads_size:continue
        structural=[f for f in rule['body'] if not any(contains(e,TRACKED|PRIMITIVES) for e in exprs(f))]
        matches=graph.query(structural)
        if not size_only:
            # Overapproximate possible future scalar values by dropping numeric
            # predicates. No structural candidate means this rule stays disabled.
            if matches:raise ValueError(f'size-dependent non-size effect needs residual execution: rule {rule["ordinal"]}')
            blocked_rules.append(rule['ordinal']);continue
        if not matches:
            # An absent projected match is not an absence certificate. Check the
            # weakened query on native tables too, including omitted containers.
            blocked_rules.append(rule['ordinal']);continue
        scalar_facts=[f for f in rule['body'] if f not in structural]
        for env in matches:
            locals_={};intermediates=[]
            for f in scalar_facts:
                if f['kind']!='eq':raise ValueError('scalar guard requires a guarded operator')
                a,b=f['args']
                if a['kind']!='var' and b['kind']=='var':a,b=b,a
                if a['kind']!='var':raise ValueError('non-binding scalar equation')
                if a['name'] in locals_ or a['name'] in env:
                    raise ValueError('repeated scalar binding is an equality guard, not assignment')
                val=scalar(b,env,locals_,graph);locals_[a['name']]=val
                intermediates.extend(scalar_subexpressions(val))
            head=heads[0]
            target=graph.resolve(head['args'][0],env)
            value=scalar(head['value'],env,locals_,graph)
            witness=dict(rule=rule['ordinal'],source_rule=rule['rule_text'],binding=env,
                         target=target,expression=value,
                         intermediates=intermediates+list(scalar_subexpressions(value)))
            certificates.append(witness);equations[target].append(value)
    seeds={}
    for _,row in graph.by_op['Expr-size']:
        v=graph.values[row['result']]
        if v['sort']!='i64' or v['literal'] is None:raise ValueError('nonconcrete seed value')
        seeds[row['args'][0]]=int(v['literal'])
    return equations,seeds,certificates,blocked_rules


def plus(a,b):
    result=dict(a)
    for k,v in b.items():result[k]=result.get(k,0)+v
    return {k:v for k,v in result.items() if v}


def scale(k,a):return {x:k*v for x,v in a.items() if k*v}


class Affine:
    def __init__(self,equations,seeds,cuts=()):
        self.equations=equations;self.seeds=seeds;self.cuts=set(cuts);self.cache={};self.visiting=set()

    def expr(self,e):
        op,args=e
        if op=='const':return {None:args} if args else {}
        if op=='field':return self.field(args)
        xs=[self.expr(a) for a in args]
        if op=='+':return plus(xs[0],xs[1])
        if op=='-':return plus(xs[0],scale(-1,xs[1]))
        if op=='*':
            if set(xs[0])<={None}:return scale(xs[0].get(None,0),xs[1])
            if set(xs[1])<={None}:return scale(xs[1].get(None,0),xs[0])
            raise ValueError('nonlinear multiplication')
        if op in {'min','max'}:
            if all(x==xs[0] for x in xs):return xs[0]
            if all(set(x)<={None} for x in xs):return {None:(min if op=='min' else max)(x.get(None,0) for x in xs)}
            raise ValueError('nontrivial min/max retained; not one affine map')
        raise ValueError(op)

    def field(self,key):
        if key in self.cuts:return {key:1}
        if key in self.cache:return self.cache[key]
        if key in self.visiting:raise ValueError('recursive scalar equations need another algebra')
        self.visiting.add(key)
        forms=[self.expr(e) for e in self.equations.get(key,[])]
        if key in self.seeds:forms.append({None:self.seeds[key]} if self.seeds[key] else {})
        if not forms:result={key:1}
        elif all(f==forms[0] for f in forms):result=forms[0]
        elif all(set(f)<={None} for f in forms):result={None:min(f.get(None,0) for f in forms)}
        else:raise ValueError(f'min envelope at field {key} is not affine')
        self.visiting.remove(key);self.cache[key]=result
        return result


def multiply(a,b):
    if (not a or not b or not b[0] or
        any(len(row)!=len(b) for row in a) or
        any(len(row)!=len(b[0]) for row in b)):
        raise ValueError('incompatible or ragged matrix dimensions')
    return [[sum(x*y for x,y in zip(row,col)) for col in zip(*b)] for row in a]
