"""Audit shared projected factors across ALL admitted Pass-3 query shapes.

This is a geometry/correctness census, not a matching engine or speed result.
Variables appearing in one atom only remain in that factor's original-row
payload. Guards and effects are residual obligations, NOT discarded semantics.
"""
import argparse
from collections import Counter,defaultdict
import gzip
import hashlib
import json
from pathlib import Path


def compile_factors(shape):
    atoms=shape['atoms'];occurs=Counter()
    for atom in atoms:
        occurs.update({c['Slot'] for c in atom['columns'] if 'Slot' in c})
    result=[]
    for atom in atoms:
        local={};columns=[]
        for c in atom['columns']:
            if 'Literal' in c:columns.append(('literal',c['Literal']))
            else:
                s=c['Slot'];local.setdefault(s,len(local));columns.append(('slot',local[s]))
        shared=[s for s in local if occurs[s]>1]
        keep=tuple(local[s] for s in shared)
        key=(atom['table'],tuple(atom['sorts']),tuple(columns),keep)
        result.append((key,shared))
    # Bipartite factor/variable forest; several binary atoms can form cycles,
    # so "at most two interface variables" does not mean "a path query".
    parent={};cycle=False
    def find(x):
        parent.setdefault(x,x)
        if parent[x]!=x:parent[x]=find(parent[x])
        return parent[x]
    for i,(_,shared) in enumerate(result):
        for s in shared:
            a,b=find(('a',i)),find(('v',s))
            if a==b:cycle=True
            else:parent[a]=b
    return result,not cycle


def typed(v):
    kind=next(iter(v));sort,x=v[kind]
    return kind,sort,str(x)


def project(rows,key):
    """Retain correlated keys and ALL original row witnesses, including aliases."""
    table,sorts,columns,keep=key;out=defaultdict(list)
    for index,row in enumerate(rows):
        if len(row)!=len(columns):raise ValueError(f'arity mismatch for {table}')
        binding={};ok=True
        for (kind,arg),sort,v in zip(columns,sorts,row):
            if v[1]!=sort:raise ValueError(f'sort mismatch for {table}')
            if kind=='literal':
                # This census uses the native printer's literal spelling.
                # Primitive guard expressions are never interpreted here.
                if v!=('Base',sort,arg):ok=False;break
            elif arg in binding and binding[arg]!=v:ok=False;break
            else:binding[arg]=v
        if ok:out[tuple(binding[s] for s in keep)].append(index)
    return out


def geometry(projection,arity):
    info={'interface_arity':arity,'projected_keys':len(projection),
          'row_payloads':sum(map(len,projection.values()))}
    if arity!=2 or not projection:return info
    axes=[{x:i for i,x in enumerate(sorted({k[c] for k in projection}))} for c in (0,1)]
    coords=[(axes[0][a],axes[1][b]) for a,b in projection]
    area=len(axes[0])*len(axes[1]);info.update({'shape':[len(a) for a in axes],
        'rectangular_slots':area,'density':len(coords)/area,'tiles':{}})
    for width in (32,64,128):
        occupied={(a//width,b//width) for a,b in coords}
        slots=len(occupied)*width*width
        info['tiles'][str(width)]={'nonempty_tiles':len(occupied),'allocated_slots':slots,
                                   'density':len(coords)/slots}
    return info


def audit(snapshot,profile):
    with gzip.open(profile,'rt') as f:header=json.loads(next(f))['header']
    saved=json.loads(snapshot.read_text());tables=defaultdict(list)
    for op,args,result,hidden in saved['rows']:
        if not hidden:tables[op].append(tuple(typed(x) for x in [*args,result]))
    del saved
    uses=defaultdict(list);queries=[]
    for qi,q in enumerate(header['queries']):
        factors,forest=compile_factors(q['shape'])
        for key,_ in factors:uses[key].append(qi)
        queries.append({'id':qi,'ruleset':q['ruleset'],'atoms':len(factors),
            'max_interface_arity':max((len(s) for _,s in factors),default=0),
            'factor_graph_is_forest':forest,'guards':len(q['shape'].get('guards',[])),
            'native_residuals':bool(q['shape'].get('native_residuals',False))})
    factors=[]
    for key,consumers in sorted(uses.items(),key=lambda x:repr(x[0])):
        projection=project(tables[key[0]],key)
        factors.append({'table':key[0],'sorts':key[1],'columns':key[2],
            'keep_local_slots':key[3],'query_ids':sorted(set(consumers)),
            'uses':len(consumers),**geometry(projection,len(key[3]))})
    binary=[x for x in factors if x['interface_arity']==2 and x['projected_keys']]
    summary={'admitted_queries':len(queries),'rejected_queries':len(header['rejected']),
        'atom_occurrences':sum(q['atoms'] for q in queries),'distinct_factors':len(factors),
        'shared_factors':sum(len(f['query_ids'])>1 for f in factors),
        'queries_only_scalar_unary_binary_factors':sum(q['max_interface_arity']<=2 for q in queries),
        'queries_only_scalar_unary_binary_factors_and_forest':sum(q['max_interface_arity']<=2 and q['factor_graph_is_forest'] for q in queries),
        'queries_with_guards':sum(q['guards']>0 for q in queries),
        'factor_arities':dict(Counter(f['interface_arity'] for f in factors)),
        'binary_nonempty_factors':len(binary),'binary_nonzeros':sum(x['projected_keys'] for x in binary),
        'binary_rectangular_slots':sum(x['rectangular_slots'] for x in binary),
        'binary_tile_slots':{str(w):sum(x['tiles'][str(w)]['allocated_slots'] for x in binary) for w in (32,64,128)}}
    return {'scope':'static shared-factor projection of all admitted query shapes on frozen canonical tables; no Run timing, no guards/RHS executed, no Cell credit',
        'snapshot_sha256':hashlib.sha256(snapshot.read_bytes()).hexdigest(),
        'profile_sha256':hashlib.sha256(profile.read_bytes()).hexdigest(),
        'summary':summary,'factors':factors,'queries':queries,
        'limits':['Per-factor sorted axes are an optimistic geometry probe, not a shared cross-factor coordinate allocation.',
                  'Local variables remain in original row payloads; materializing only projected keys loses full bindings.',
                  'Guards may connect projected-away variables; exact joint payload validation remains necessary.',
                  'A forest result concerns only atom equalities before guard residuals.',
                  'Small interface arity does not imply low rank, dense tiles, or inexpensive live maintenance.']}


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--snapshot',required=True,type=Path)
    p.add_argument('--profile',required=True,type=Path);p.add_argument('--output',required=True,type=Path)
    a=p.parse_args();result=audit(a.snapshot,a.profile)
    a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result['summary'],indent=2))
