#!/usr/bin/env python3
"""Exact cover of required comparisons by paired kernels / rectangular GEMMs.
Only physical plans are selected. Work-unit costs are explicit, not timings.
"""
import argparse
import contextlib
import json
import math
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.cell_spine import solver

DEFAULT_COSTS = dict(dispatch=128.0, scalar=1.0, gemm_mac=0.125, word=0.01)

def candidates(groups, costs):
    result = []
    for group in groups:
        edges = {i:(a,b) for i,a,b in group['edges']}
        width = group['width']
        parent = {}
        def root(x):
            parent.setdefault(x,x)
            while parent[x] != x:
                parent[x] = parent[parent[x]]; x=parent[x]
            return x
        for a,b in edges.values(): parent[root(a)] = root(b)
        components={}; left={}; right={}
        for i,(a,b) in edges.items():
            components.setdefault(root(a),[]).append(i)
            left.setdefault(a,[]).append(i);right.setdefault(b,[]).append(i)
        families={}
        for ids in components.values():
            # Same execution dimensions can share a paired dispatch. This pools
            # work only; it never equates values, bindings, or Cell semantics.
            shape=(len(ids),len({edges[i][0] for i in ids}),len({edges[i][1] for i in ids}))
            families.setdefault(shape,[]).extend(ids)
        # Polynomial vocabulary. No subset enumeration, candidate truncation or
        # time limit. The model is exact over this declared vocabulary only.
        sets={tuple(sorted(s)) for s in [list(edges),*components.values(),*families.values(),*left.values(),*right.values(),*([i] for i in edges)]}
        for ids in sorted(sets):
            ls={edges[i][0] for i in ids};rs={edges[i][1] for i in ids}
            n=len(ids);l=len(ls);r=len(rs)
            for method in ['paired','rectangular']:
                if method=='paired':
                    operations=n*(3*width+1);words=2*n*width
                    price=3*costs['dispatch']+operations*costs['scalar']+words*costs['word']
                    positions=n;scratch=0
                else:
                    operations=l*r*width;words=(l+r)*width+l*r
                    price=5*costs['dispatch']+operations*costs['gemm_mac']+4*l*r*costs['scalar']+words*costs['word']
                    positions=l*r;scratch=(l+r)*width+l*r
                result.append(dict(method=method,conditions=list(ids),price=price,
                    width=width,left=l,right=r,positions=positions,scratch_words=scratch))
    return result

def solve(request, costs):
    cs=candidates(request['groups'],costs)
    required={e[0] for g in request['groups'] for e in g['edges']}
    with solver.Model('matrix_layout') as model:
        xs=[model.addVar(vtype=solver.BINARY,name=f'block_{i}') for i in range(len(cs))]
        covers={c:[] for c in required}
        for x,b in zip(xs,cs):
            for c in b['conditions']:covers[c].append(x)
        for c in sorted(required):model.addConstr(solver.quicksum(covers[c])==1)
        model.setObjective(solver.quicksum(x*b['price'] for x,b in zip(xs,cs)))
        model.optimize()
        if model.Status!=solver.OPTIMAL:raise RuntimeError(f'layout solve is {model.Status}')
        chosen=[b for x,b in zip(xs,cs) if model.value(x)>0.5]
        objective=model.ObjVal
    return dict(blocks=[dict(method=b['method'],conditions=b['conditions']) for b in chosen],
        report=dict(solver=solver.statistics(),cost_units='reference work units, not nanoseconds',
            costs=costs,objective=objective,candidates=len(cs),required_conditions=len(required),selected=chosen,
            vocabulary='whole shape, connected component, equal-dimension component batch, left/right star, singleton; each paired or rectangular',
            limitations='full-demand estimate; shared feature production is constant and cached by the executor; no numeric-layout, marker-view or arbitrary-partition optimization'))

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--solver',choices=['scip','gurobi'],default='scip');p.add_argument('--costs',type=Path)
    a=p.parse_args();r=json.load(sys.stdin)
    if r.get('version')!=1:raise ValueError('layout protocol')
    costs=DEFAULT_COSTS if a.costs is None else json.loads(a.costs.read_text())
    if set(costs)!=set(DEFAULT_COSTS) or any(not math.isfinite(v) or v<0 for v in costs.values()):raise ValueError('invalid explicit layout cost model')
    with contextlib.redirect_stdout(sys.stderr):
        solver.select(a.solver);result=solve(r,costs)
    json.dump(result,sys.stdout)

if __name__=='__main__':main()
