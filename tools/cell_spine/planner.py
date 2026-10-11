"""Global binary path selection. No greedy fallback or candidate budget."""
from collections import defaultdict


def solve(candidates, edges, output, weights=(1., 1., 1.)):
    import gurobipy as gp
    groups = defaultdict(list)
    for c in candidates:
        groups[c['family']].append(c)
    with gp.Model('cell_spine') as model:
        model.Params.OutputFlag = 0
        model.Params.MIPGap = 0
        model.Params.Threads = 1
        x = {c['id']: model.addVar(vtype=gp.GRB.BINARY, name=f"path_{c['id']}") for c in candidates}
        for f, paths in groups.items():
            model.addConstr(gp.quicksum(x[p['id']] for p in paths) == 1, name=f'one_path_{f}')
        users = defaultdict(list)
        for c in candidates:
            for node in c['nodes']:
                users[node].append(x[c['id']])
        ys = []
        for i, uses in enumerate(users.values()):
            y = model.addVar(vtype=gp.GRB.BINARY, name=f'shared_node_{i}')
            ys.append(y)
            for u in uses:
                model.addConstr(y >= u)
            model.addConstr(y <= gp.quicksum(uses))
        # One compiled implementation per observed hyperedge. A choice jointly
        # requires ALL endpoint representations, not independent pairwise edges.
        z, options = {}, {}
        for edge in edges:
            terms = []
            for opt in edge['options']:
                v = model.addVar(vtype=gp.GRB.BINARY, name=opt['id'])
                z[opt['id']] = v
                options[opt['id']] = opt
                terms.append(v)
                for cid in opt['requires']:
                    model.addConstr(v <= x[cid], name=f'{opt["id"]}_needs_{cid}')
            model.addConstr(gp.quicksum(terms) == 1, name=f'one_impl_{edge["id"]}')
        storage = gp.quicksum(ys)
        search = gp.quicksum(options[k]['search_atoms'] * v for k, v in z.items())
        update = gp.quicksum(options[k]['update_actions'] * v for k, v in z.items())
        model.setObjective(weights[0] * storage + weights[1] * search + weights[2] * update)
        model.write(str(output / 'selection.lp'))
        model.optimize()
        if model.Status != gp.GRB.OPTIMAL:
            raise RuntimeError(f'Gurobi did not prove optimality: status {model.Status}')
        model.write(str(output / 'selection.sol'))
        selected = [c['id'] for c in candidates if x[c['id']].X > .5]
        return dict(selected=selected, implementations=[k for k,v in z.items() if v.X > .5],
                    objective=model.ObjVal, bound=model.ObjBound,
                    costs=dict(template_nodes=storage.getValue(), lookup_atoms_and_guards=search.getValue(),
                               write_atoms_and_unions=update.getValue()),
                    hyperedges=len(edges), implementations_considered=len(z),
                    status='optimal', variables=model.NumVars, constraints=model.NumConstrs,
                    weights=dict(storage=weights[0], search=weights[1], update=weights[2]),
                    cost_scope='shared template shapes + distinct nested lookup terms/guards + write terms/unions of compiled implementations; not measured Join work, runtime or RSS')
