"""Compile native terms cost expressions to immutable affine columns + i64 DAG.

The small adapter recognizes the existing TCPair/ExtractedExpr interface, not
handwritten Bop/Top cost formulas. Full correlated candidate tuples are kept.
"""
from collections import defaultdict
import itertools
try:
    from .features import identity
except ImportError:
    from features import identity

from eggfuse.compiler.recipes.affine import LO,HI,compile_program,exact

def pair_index(snapshot):
    pairs=defaultdict(set);functions={}
    for row in snapshot['rows']:
        if row['subsumed']:continue
        v=row['values']
        if row['table']=='TCPair':pairs[identity(v[2])].add((identity(v[0]),int(v[1]['value'])))
        if row['table']=='ExtractedExpr':functions[identity(v[0])]=identity(v[1])
    return {e:sorted(pairs[p]) for e,p in functions.items()},functions


def contexts(snapshot,keys,program):
    import numpy as np
    lookup,functions=pair_index(snapshot);rootids={k:i for i,k in enumerate(keys)};rows=defaultdict(list)
    for r in snapshot['rows']:
        if not r['subsumed']:rows[r['table']].append(r)
    params=[];meta=[];witnesses=[]
    for rid,recipe in enumerate(program['recipes']):
        for row in rows[recipe['anchor']]:
            values=list(map(identity,row['values']));root=values[-1]
            if root not in rootids:continue
            binding={};ok=True
            for term,value in zip(recipe['args'],values):
                name=term['var']
                if name in binding and binding[name]!=value:ok=False
                binding[name]=value
            if not ok:continue
            choices=[lookup.get(values[p['column']]) or [None] for p in recipe['inputs']]
            old=lookup.get(root) or [None]
            for selected in itertools.product(*choices):
                # A repeated source variable must have the same correlated
                # term/cost witness; unrelated row halves are never combined.
                b=dict(binding);valid=True;costs=[0,0,0];complete=True
                for i,(p,choice) in enumerate(zip(recipe['inputs'],selected)):
                    if choice is None:complete=False;continue
                    t,c=choice
                    for name,value in [(p['term'],t),(p['cost'],('i64','literal',str(c))),(p['return'],functions[values[p['column']]])]:
                        if name in b and b[name]!=value:valid=False
                        b[name]=value
                    costs[i]=c
                if not valid:continue
                for current in old:
                    params.append([*costs,current[1] if current else 0])
                    meta.append([rootids[root],rid,int(complete),int(current is not None)])
                    witnesses.append((tuple(values),tuple(selected),current))
    return np.array(params,np.int64).reshape(-1,4),np.array(meta,np.uint32).reshape(-1,4),witnesses,lookup


def metal_program(program):
    """Generate exact checked-i64 instructions, preserving source association."""
    out=['void evaluate(uint recipe, thread long *v, thread bool &ok, thread long &value) { switch(recipe) {']
    for i,r in enumerate(program['recipes']):
        out.append(f'case {i}: {{')
        for j,(op,ak,a,bk,b) in enumerate(r['operations']):
            aa=f'as_type<long>(ulong({a & ((1<<64)-1)}ul))' if ak else f'v[{a}]'
            bb=f'as_type<long>(ulong({b & ((1<<64)-1)}ul))' if bk else f'v[{b}]'
            out.append(f'v[{j+3}] = checked({aa},{bb},{op},ok);')
        kind,v=r['result'];value=f'as_type<long>(ulong({v & ((1<<64)-1)}ul))' if kind else f'v[{v}]'
        out.append(f'value={value}; break; }}')
    out.append('default:ok=false;break;} }')
    return '\n'.join(out)
