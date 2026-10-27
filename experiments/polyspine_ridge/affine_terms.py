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

LO=-(1<<63);HI=(1<<63)-1


def compile_program(core):
    rules=next(r['rules'] for r in core['rulesets'] if r['name']=='terms');out=[]
    for r in rules:
        anchors=[a for a in r['body'] if a['call'].get('table') not in ['TCPair','ExtractedExpr']]
        if len(anchors)!=1 or 'table' not in anchors[0]['call']:raise ValueError('unsupported structural anchor')
        anchor=anchors[0];cols={a['var']:i for i,a in enumerate(anchor['args'])}
        extracts={a['args'][-1]['var']:a['args'][0]['var'] for a in r['body'] if a['call'].get('table')=='ExtractedExpr'}
        inputs=[]
        for a in r['body']:
            if a['call'].get('table')=='TCPair':
                term,cost,ret=a['args'];key=extracts[ret['var']]
                inputs.append({'column':cols[key],'key':key,'term':term['var'],'cost':cost['var'],'return':ret['var']})
        if len(inputs)>3:raise ValueError('packet supports up to three independent cost inputs')
        refs={p['cost']:(0,i) for i,p in enumerate(inputs)}
        affine={p['cost']:[int(j==i) for j in range(4)] for i,p in enumerate(inputs)}
        ops=[];final=None
        def ref(a):return (1,int(a['literal'])) if 'literal' in a else refs[a['var']]
        def linear(a):return [0,0,0,int(a['literal'])] if 'literal' in a else affine[a['var']]
        for a in r['head']:
            if a['op']=='copy':
                if a['arg']['var'] in refs:refs[a['dst']]=refs[a['arg']['var']];affine[a['dst']]=affine[a['arg']['var']]
            elif a['op']=='let' and 'primitive' in a['call']:
                op=a['call']['primitive']
                if op not in ['+','-'] or a['call']['output']!='i64':raise ValueError('non-affine primitive')
                left,right=map(ref,a['args']);ops.append([0 if op=='+' else 1,*left,*right])
                refs[a['dst']]=(0,3+len(ops)-1)
                aa,bb=map(linear,a['args']);affine[a['dst']]=[x+y if op=='+' else x-y for x,y in zip(aa,bb)]
            elif a['op']=='let' and a['call'].get('table')=='TCPair':
                final=ref(a['args'][1]);coeff=list(linear(a['args'][1]))
        if final is None:raise ValueError('missing TCPair result')
        # For admitted formulas, |each input|<=2048 makes the two final
        # affine columns exact in FP32. The original i64 instruction DAG is
        # checked separately so reassociation cannot hide intermediate overflow.
        bound=sum(abs(c)*2048 for c in coeff[:3])+abs(coeff[3])
        safe_coeff=all(abs(c)<=2048 for c in coeff) and bound+2048<(1<<24)
        out.append({'name':r['name'],'anchor':anchor['call']['table'],'args':anchor['args'],'inputs':inputs,
                    'coefficients':coeff,'operations':ops,'result':final,'matrix_range_certificate':safe_coeff})
    return {'schema':'native-affine-terms/v1','recipes':out,'max_registers':3+max(len(r['operations']) for r in out)}


def exact(recipe,costs,cold,complete=True,has_old=True):
    if not complete:return 0,0,0
    regs=list(map(int,costs));valid=True
    for op,ak,a,bk,b in recipe['operations']:
        av=a if ak else regs[a];bv=b if bk else regs[b];v=av+bv if op==0 else av-bv
        if not LO<=v<=HI:valid=False;break
        regs.append(v)
    if not valid:return 0,0,0
    kind,v=recipe['result'];new=v if kind else regs[v];delta=int(cold)-new
    flags=1 | (2 if has_old and new<cold else 0) | (4 if not has_old else 0) | (8 if new<0 else 0)
    if has_old and LO<=delta<=HI:flags|=16
    else:delta=0
    if recipe['matrix_range_certificate'] and all(abs(int(c))<=2048 for c in [*costs,cold]):flags|=32
    return new,delta,flags


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
