"""Learn typed merge descriptions from witnessed transitions, not operator names.

Anti-unification introduces shared holes for repeated differences across an
entire transition (sources, target, obligations). MIP selects a minimum-size
dictionary plus residual bindings. This compresses evidence; it is NOT an
induction proof or permission to execute an unseen transition.
"""
import hashlib
import itertools
import json
from collections import defaultdict


def key(x):return json.dumps(x,sort_keys=True,separators=(',',':'))
def digest(x):return hashlib.sha256(key(x).encode()).hexdigest()
def size(x):
    if isinstance(x,dict):return 1+sum(size(v) for v in x.values())
    if isinstance(x,list):return 1+sum(size(v) for v in x)
    return 1


def typ(x):
    if isinstance(x,dict) and 'sort' in x:return x['sort']
    return type(x).__name__


def generalize(a,b):
    """Least shared tree context; repeated differing pairs share one variable.

    Port identity is a structural value, not an independent wildcard at every
    occurrence. Whole term/sequence holes keep their original binding as data.
    """
    holes={}
    def hole(x,y):
        if typ(x)!=typ(y):raise ValueError('incompatible typed structures')
        token=(key(x),key(y));i=holes.setdefault(token,len(holes))
        return {'$hole':i,'sort':typ(x)}
    def visit(x,y):
        if x==y:return x
        if isinstance(x,dict) and isinstance(y,dict) and x.keys()==y.keys():
            if 'sort' in x and ('port' in x or 'identity' in x):return hole(x,y)
            if 'sort' in x and (x['sort']!=y['sort'] or x.get('op')!=y.get('op')):return hole(x,y)
            return {k:visit(x[k],y[k]) for k in x}
        if isinstance(x,list) and isinstance(y,list) and len(x)==len(y):return [visit(u,v) for u,v in zip(x,y)]
        return hole(x,y)
    return visit(a,b)


def bindings(pattern,value):
    result={}
    def visit(p,v):
        if isinstance(p,dict) and '$hole' in p:
            if p['sort']!=typ(v):return False
            i=str(p['$hole'])
            if i in result:return result[i]==v
            result[i]=v;return True
        if type(p)!=type(v):return False
        if isinstance(p,dict):return p.keys()==v.keys() and all(visit(p[k],v[k]) for k in p)
        if isinstance(p,list):return len(p)==len(v) and all(visit(x,y) for x,y in zip(p,v))
        return p==v
    return result if visit(pattern,value) else None


def instantiate(pattern,values):
    if isinstance(pattern,dict):
        if '$hole' in pattern:return values[str(pattern['$hole'])]
        return {k:instantiate(v,values) for k,v in pattern.items()}
    if isinstance(pattern,list):return [instantiate(x,values) for x in pattern]
    return pattern


def parameters(pattern):
    found={}
    def visit(x):
        if isinstance(x,dict):
            if '$hole' in x:found[x['$hole']]=dict(name=f'h{x["$hole"]}',sort=x['sort'])
            else:
                for v in x.values():visit(v)
        elif isinstance(x,list):
            for v in x:visit(v)
    visit(pattern)
    return [found[i] for i in sorted(found)]


def describe(pattern):
    def show(e):
        if isinstance(e,dict):
            if '$hole' in e:return f'?h{e["$hole"]}:{e["sort"]}'
            if 'op' in e:return '('+' '.join([e['op']]+arguments(e['args']))+')'
            if e.get('sort')=='CellState':return 'Cell['+str(e['identity'])[:10]+']'
            if 'port' in e:return '$'+show(e['port'])
            if 'literal' in e:return str(e['literal'])
            if 'kind' in e:return '('+' '.join([show(e['kind'])]+arguments(e['terms']))+')'
            return key(e)
        if isinstance(e,list):return '['+', '.join(show(x) for x in e)+']'
        return str(e)
    def arguments(xs):return [show(x) for x in xs] if isinstance(xs,list) else [show(xs)]
    state=pattern.get('states');lines=[]
    if state:lines.append('states:  '+show(state['sources'])+' -> '+show(state['target']))
    return '\n'.join(lines+['sources: '+show([p['roots'] for p in pattern['parents']]),
                       'target:  '+show(pattern['target']['roots']),
                       'requires: '+show(pattern['external'])])


def learn(transitions,previous=()):
    """Incrementally propose from observed envelopes; never enumerate programs.

    Exactly one encoding covers each distinct observed transition. A shared
    pattern is useful only if its definition plus call bindings is smaller.
    Previously selected patterns also take part when new examples arrive.
    """
    from . import solver as gp
    groups=defaultdict(list);patterns={}
    def add(pattern):patterns.setdefault(digest(pattern),pattern)
    for tid,r in sorted(transitions.items()):
        add(r['template']);groups[key(r['bucket'])].append(tid)
    for p in previous:add(p['template'])
    for members in groups.values():
        for a,b in itertools.combinations(members,2):
            try:add(generalize(transitions[a]['template'],transitions[b]['template']))
            except ValueError:pass
    # Re-generalize previous dictionary entries against new evidence too. This
    # widens a learned context without changing the type/effect witness records.
    for p in previous:
        for r in transitions.values():
            if p.get('bucket')!=r['bucket']:continue
            try:add(generalize(p['template'],r['template']))
            except ValueError:pass
    cover={};by_transition=defaultdict(list)
    for pid,p in patterns.items():
        matches={tid:b for tid,r in transitions.items() if (b:=bindings(p,r['template'])) is not None}
        for tid,b in matches.items():
            if instantiate(p,b)!=transitions[tid]['template']:raise ValueError('merge template failed exact expansion')
            by_transition[tid].append(pid)
        if matches:cover[pid]=matches
    if not transitions:return dict(families=[],objective=0,variables=0,constraints=0)
    with gp.Model('merge_family_dictionary') as model:
        selected={p:model.addVar(vtype=gp.BINARY,name=f'family_{i}') for i,p in enumerate(cover)}
        assignment={}
        for i,(pid,matches) in enumerate(cover.items()):
            for j,tid in enumerate(matches):
                v=model.addVar(vtype=gp.BINARY,name=f'use_{i}_{j}');assignment[pid,tid]=v
                model.addConstr(v<=selected[pid])
            model.addConstr(selected[pid]<=gp.quicksum(assignment[pid,t] for t in matches))
        for tid,ps in by_transition.items():model.addConstr(gp.quicksum(assignment[p,tid] for p in ps)==1)
        model.setObjective(gp.quicksum(size(patterns[p])*v for p,v in selected.items())+
            gp.quicksum((1+sum(size(x) for x in cover[p][t].values()))*v for (p,t),v in assignment.items()))
        model.optimize()
        if model.Status!=gp.OPTIMAL:raise RuntimeError('merge-family dictionary did not solve to optimality')
        families=[]
        for pid,matches in cover.items():
            if model.value(selected[pid])<.5:continue
            tids=[t for t in matches if model.value(assignment[pid,t])>.5]
            p=patterns[pid];ps=parameters(p)
            samples=sorted({s for t in tids for s in transitions[t]['samples']})
            first=transitions[tids[0]]
            families.append(dict(id=pid,template=p,parameters=ps,bucket=first['bucket'],selected=True,
                pattern=describe(p),
                status='generalized-hypothesis' if ps else 'observed',
                summary=f'{len(first["template"]["parents"])} source Cell(s), {len(tids)} distinct transition(s), {len(ps)} shared parameter(s)',
                transitions=tids,samples=samples,support_samples=len(samples),verified_instances=len(tids),
                definition_nodes=size(p),encoded_nodes=size(p)+sum(1+sum(size(x) for x in matches[t].values()) for t in tids),
                explicit_nodes=sum(size(transitions[t]['template']) for t in tids),
                scope='exact expansion of observed merge descriptions; no arbitrary-size recurrence proof or runtime shortcut'))
        return dict(families=sorted(families,key=lambda f:(-f['support_samples'],-f['explicit_nodes']+f['encoded_nodes'],f['id'])),
                    objective=model.ObjVal,variables=model.NumVars,constraints=model.NumConstrs)
