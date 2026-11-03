"""Exact existing-row projections for constructor results; not a new cache key."""
from .multistage import sources,symbol

def build(recipe):
    tables={t['name']:t for t in recipe['tables']};plans=[]
    for rid,r in enumerate(recipe['rules']):
        rows=sources(r,tables)
        for op,e in enumerate(r['effects']):
            if e['kind']!='construct':continue
            target=next(x for x in rows if x['kind']=='effect' and x['index']==op)
            for source in rows:
                if source['kind']=='effect' and source['index']>=op:continue
                if source['table']==target['table'] and tuple(map(symbol,source['args'][:-1]))==tuple(map(symbol,target['args'][:-1])):
                    plans.append(dict(rule=rid,operation=op,source_kind=source['kind'],source_index=source['index']));break
    return dict(schema='eggfuse-known-results/v1',recipe_sha256=recipe['sha256'],plans=plans,static_aliases=static_aliases(recipe),
        contract='live canonical FactRef with identical full constructor key; dirty domains fall back; never skip set/union/guard')

def static_aliases(recipe):
    tables={t['name']:t for t in recipe['tables']}
    deleted={e['call']['table'] for r in recipe['rules'] for e in r['effects'] if e['op']=='delete'}
    result=[]
    for rid,r in enumerate(recipe['rules']):
        aliases={};known={}
        def norm(a):
            while 'slot' in a and a['slot'] in aliases:a=aliases[a['slot']]
            return a
        def key(t,args):return t,tuple(symbol(norm(a)) for a in args)
        for a in r['query']:
            t=a['call'].get('table')
            if t in tables and tables[t]['kind']=='constructor' and t not in deleted:
                known.setdefault(key(t,a['args'][:-1]),a['args'][-1])
        for i,e in enumerate(r['effects']):
            if e['op']=='copy':aliases[e['dst']]=norm(e['arg'])
            elif e['op']=='delete':known={k:v for k,v in known.items() if k[0]!=e['call']['table']}
            elif e['kind']=='construct':
                t=e['call']['table'];k=key(t,e['args'])
                if k in known:
                    value=norm(known[k]);aliases[e['dst']]=value
                    result.append(dict(rule=rid,operation=i,dst=e['dst'],value=value))
                else:known[k]={'slot':e['dst']}
    return result
