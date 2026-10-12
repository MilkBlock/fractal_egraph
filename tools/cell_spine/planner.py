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
        # Marker placement is coupled to the same path and hyperedge choices.
        # Each layout states exactly which fields are stored and which
        # canonical subtree must be read to recover the remaining fields.
        markers={}
        for c in candidates:
            layouts=c.get('marker_layouts',[])
            if not layouts:continue
            for layout in layouts:
                markers[layout['id']]=model.addVar(vtype=gp.GRB.BINARY,name=layout['id'])
            model.addConstr(gp.quicksum(markers[l['id']] for l in layouts)==x[c['id']],name=f'marker_path_{c["id"]}')
        marker_storage=gp.quicksum(l['columns']*markers[l['id']] for c in candidates for l in c.get('marker_layouts',[]))
        marker_search=[];marker_update=[]
        # Factor sum_o(m_layout * z_o * uses_o) into
        # m_layout * sum_o(z_o * uses_o). This is exactly the same objective,
        # with one product per path/layout instead of per edge option/layout.
        for c in candidates:
            def uses(opt,mode):
                return (sum(p['path']==c['id'] for p in opt.get('parents',[]))
                        if mode=='read' else int(opt.get('target_path')==c['id']))
            for mode,accumulator,price in [('read',marker_search,'lookup_atoms'),('write',marker_update,'columns')]:
                total=gp.quicksum(uses(opt,mode)*z[key] for key,opt in options.items())
                bound=sum(max((uses(opt,mode) for opt in edge['options']),default=0) for edge in edges)
                if not bound:continue
                for layout in c.get('marker_layouts',[]):
                    if not layout[price]:continue
                    m=markers[layout['id']]
                    product=model.addVar(lb=0,ub=bound,name=f'{mode}_{layout["id"]}')
                    model.addConstr(product<=bound*m)
                    model.addConstr(product<=total)
                    model.addConstr(product>=total-bound*(1-m))
                    accumulator.append(layout[price]*product)
        storage = gp.quicksum(ys)
        search = gp.quicksum(options[k]['search_atoms'] * v for k, v in z.items())+gp.quicksum(marker_search)
        update = gp.quicksum(options[k]['update_actions'] * v for k, v in z.items())+gp.quicksum(marker_update)
        model.setObjective(weights[0] * (storage+marker_storage) + weights[1] * search + weights[2] * update)
        model.write(str(output / 'selection.lp'))
        model.optimize()
        if model.Status != gp.GRB.OPTIMAL:
            raise RuntimeError(f'Gurobi did not prove optimality: status {model.Status}')
        model.write(str(output / 'selection.sol'))
        selected = [c['id'] for c in candidates if x[c['id']].X > .5]
        return dict(selected=selected, implementations=[k for k,v in z.items() if v.X > .5],
                    marker_layouts=[k for k,v in markers.items() if v.X > .5],
                    objective=model.ObjVal, bound=model.ObjBound,
                    costs=dict(template_nodes=storage.getValue(), marker_columns=marker_storage.getValue(), lookup_atoms_and_guards=search.getValue(),
                               write_atoms_and_unions=update.getValue()),
                    hyperedges=len(edges), implementations_considered=len(z),
                    status='optimal', variables=model.NumVars, constraints=model.NumConstrs,
                    weights=dict(storage=weights[0], search=weights[1], update=weights[2]),
                    cost_scope='structural estimate: shared path shapes, marker columns, seed decoder lookups on observed parent uses, and writes; inherited consumers and decoder elimination are reported after lowering; optimum is for this model, not measured Join work or RSS')
