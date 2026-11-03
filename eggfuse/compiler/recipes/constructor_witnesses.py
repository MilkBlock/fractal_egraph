"""Reuse immutable constructor facts already witnessed by one typed invocation.

The batch engine validates descendants against actual rows and defers unions
until its block boundary. Default union constructors retain their witnessed
value during that action phase. Mutable/custom merge policies are excluded.
"""
from eggfuse.ir.recipe import digest


def constructor_witnesses(rule,tables):
    tables={t['name']:t for t in tables};facts={};aliases={};result={}
    def resolve(x):
        while 'slot' in x and x['slot'] in aliases:x=aliases[x['slot']]
        return x
    def key(name,args):return name,tuple(digest(resolve(a)) for a in args)
    for i,a in enumerate(rule['query']):
        name=a['call'].get('table');t=tables.get(name)
        if t and t['kind']=='constructor' and t.get('merge') is None:
            facts[key(name,a['args'][:-1])]=dict(atom=i,value=a['args'][-1])
    for i,e in enumerate(rule['effects']):
        if e['kind']=='copy':aliases[e['dst']]=resolve(e['arg']);continue
        if e['kind'] not in ('construct','set'):continue
        name=e['call']['table'];proof=facts.get(key(name,e['args']))
        if proof is None:continue
        result[i]=dict(proof,table=name,contract='same typed constructor key; actual LHS value; default union merge; block-deferred unions')
        if e['kind']=='construct':aliases[e['dst']]=resolve(proof['value'])
    return result
