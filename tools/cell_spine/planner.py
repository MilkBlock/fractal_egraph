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
        update = gp.LinExpr()
        for i, (a, b) in enumerate(sorted(set(edges))):
            if a == b or a not in groups or b not in groups:
                continue
            for p in groups[a]:
                for q in groups[b]:
                    z = model.addVar(vtype=gp.GRB.BINARY, name=f'edge_{i}_{p["id"]}_{q["id"]}')
                    model.addConstr(z <= x[p['id']]); model.addConstr(z <= x[q['id']])
                    model.addConstr(z >= x[p['id']] + x[q['id']] - 1)
                    update += len(set(q['nodes']) - set(p['nodes'])) * z
        search = gp.quicksum(c['lhs_atoms'] * x[c['id']] for c in candidates)
        model.setObjective(weights[0] * gp.quicksum(ys) + weights[1] * search + weights[2] * update)
        model.write(str(output / 'selection.lp'))
        model.optimize()
        if model.Status != gp.GRB.OPTIMAL:
            raise RuntimeError(f'Gurobi did not prove optimality: status {model.Status}')
        model.write(str(output / 'selection.sol'))
        selected = [c['id'] for c in candidates if x[c['id']].X > .5]
        return dict(selected=selected, objective=model.ObjVal, bound=model.ObjBound,
                    status='optimal', variables=model.NumVars, constraints=model.NumConstrs,
                    weights=dict(storage=weights[0], search=weights[1], update=weights[2]),
                    cost_scope='template structure/code and query-atom proxies; not measured runtime or RSS')
