"""Exact conditional optimization of paths and emitted marker programs.

The master never freezes a spine greedily. A subproblem returns the *minimum*
marker/runtime cost for that complete path/implementation choice. Value cuts
feed this back until the master lower bound meets a feasible incumbent.
"""
from collections import defaultdict
import math


def solve(candidates, edges, output, weights=(1.,1.,1.), *, pricing):
    import gurobipy as gp
    groups=defaultdict(list)
    for c in candidates:groups[c['family']].append(c)
    by_id={c['id']:c for c in candidates}
    minimum_columns={c['id']:min((l['columns'] for l in c.get('marker_layouts',[])),default=0) for c in candidates}
    with gp.Model('cell_spine_master') as model:
        model.Params.OutputFlag=0;model.Params.MIPGap=0;model.Params.MIPGapAbs=0;model.Params.Threads=1
        x={c['id']:model.addVar(vtype=gp.GRB.BINARY,name=f'path_{c["id"]}') for c in candidates}
        for family,paths in groups.items():model.addConstr(gp.quicksum(x[c['id']] for c in paths)==1,name=f'one_path_{family}')
        users=defaultdict(list)
        for c in candidates:
            for node in c['nodes']:users[node].append(x[c['id']])
        ys=[]
        for i,uses in enumerate(users.values()):
            y=model.addVar(vtype=gp.GRB.BINARY,name=f'shared_node_{i}');ys.append(y)
            for u in uses:model.addConstr(y>=u)
            model.addConstr(y<=gp.quicksum(uses))
        z={}
        for edge in edges:
            for opt in edge['options']:
                v=model.addVar(vtype=gp.GRB.BINARY,name=opt['id']);z[opt['id']]=v
                for cid in opt['requires']:model.addConstr(v<=x[cid],name=f'{opt["id"]}_needs_{cid}')
            model.addConstr(gp.quicksum(z[o['id']] for o in edge['options'])==1,name=f'one_impl_{edge["id"]}')
        extra=model.addVar(lb=0,name='conditional_runtime_cost')
        template_storage=gp.quicksum(ys)
        marker_bound=gp.quicksum(minimum_columns[cid]*v for cid,v in x.items())
        model.setObjective(weights[0]*(template_storage+marker_bound)+extra)
        incumbent=None;history=[];visited=set();inner_vars=0;inner_constraints=0
        while True:
            model.optimize()
            if model.Status!=gp.GRB.OPTIMAL:raise RuntimeError(f'path master not solved: {model.Status}')
            bound=model.ObjVal
            if incumbent is not None and (bound>=incumbent['objective'] or
                                          math.isclose(bound,incumbent['objective'],rel_tol=1e-9,abs_tol=0)):
                break
            plan=dict(selected=[cid for cid,v in x.items() if v.X>.5],implementations=[oid for oid,v in z.items() if v.X>.5])
            key=tuple(plan['selected']),tuple(plan['implementations'])
            if key in visited:raise RuntimeError('master returned a priced scheme below its certified value')
            visited.add(key)
            _,value=pricing.optimize(plan)
            inner_vars=max(inner_vars,value['variables']);inner_constraints=max(inner_constraints,value['constraints'])
            nodes=len({node for cid in plan['selected'] for node in by_id[cid]['nodes']})
            objective=weights[0]*nodes+value['value']
            history.append(dict(selected=plan['selected'],implementations=plan['implementations'],master_bound=bound,
                                conditional_value=value['value'],objective=objective))
            if incumbent is None or objective<incumbent['objective']:
                incumbent=dict(plan,objective=objective,marker_layouts=value['marker_layouts'],
                    costs=dict(template_nodes=nodes,**value['costs']),pricing=value)
            # For this exact path/implementation tuple the remaining cost is
            # known. For every other integer tuple the RHS is <= 0. This cut
            # removes no candidate and requires no guessed big-M coefficient.
            residual=value['value']-weights[0]*sum(minimum_columns[cid] for cid in plan['selected'])
            if residual < -1e-7:raise ValueError('invalid lower bound on marker storage')
            decisions=[x[cid] for cid in plan['selected']]+[z[oid] for oid in plan['implementations']]
            model.addConstr(extra>=max(0.,residual)*(gp.quicksum(decisions)-len(decisions)+1),name=f'priced_scheme_{len(history)}')
        model.write(str(output/'selection.lp'))  # Unrestricted master plus valid value cuts.
        master_vars=model.NumVars;master_constraints=model.NumConstrs
        # Save a solution for the chosen incumbent, even if the final master
        # lower bound was attained by another tied/unpriced spine assignment.
        for cid,v in x.items():v.LB=v.UB=float(cid in incumbent['selected'])
        for oid,v in z.items():v.LB=v.UB=float(oid in incumbent['implementations'])
        model.optimize()
        if model.Status!=gp.GRB.OPTIMAL or not math.isclose(model.ObjVal,incumbent['objective'],rel_tol=1e-8,abs_tol=1e-6):
            raise ValueError('incumbent is inconsistent with the exact value cuts')
        model.write(str(output/'selection.sol'))
        return dict(incumbent,status='optimal',bound=bound,
            variables=max(master_vars,inner_vars),constraints=max(master_constraints,inner_constraints),
            model_size_scope='maximum per model, not sum across independent solves',
            master=dict(variables=master_vars,constraints=master_constraints),
            inner_peak=dict(variables=inner_vars,constraints=inner_constraints),
            priced_schemes=len(history),scheme_history=history,
            hyperedges=len(edges),implementations_considered=len(z),
            weights=dict(storage=weights[0],search=weights[1],update=weights[2]),
            cost_scope='shared template shapes + marker columns + actual lowered runtime lookups/writes/marker keys, including inheritance and retirement; structural work per emitted rule, not runtime frequency or RSS')
