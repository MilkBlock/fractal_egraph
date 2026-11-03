"""Typed, name-independent shared entry nodes and source-gated consumers."""
import copy,json

SCHEMA='eggfuse-entry-schedule/v1'

def build(recipe):
    nodes=[];ids={}
    for rid,rule in enumerate(recipe['rules']):
        if not rule['metadata'].get('seminaive',True):continue
        for atom,a in enumerate(rule['query']):
            if 'table' not in a['call']:continue
            slots={};args=[];projection=[]
            for col,v in enumerate(a['args']):
                if 'slot' in v:
                    if v['slot'] not in slots:
                        slots[v['slot']]=len(slots)
                        projection.append(dict(slot=v['slot'],port=slots[v['slot']]))
                    args.append(dict(slot=slots[v['slot']]))
                else:args.append(copy.deepcopy(v))
            table=a['call']['table'];key=json.dumps([table,args],sort_keys=True)
            if key not in ids:
                ids[key]=len(nodes)
                nodes.append(dict(id=len(nodes),table=table,args=args,ports=len(slots),consumers=[]))
            nodes[ids[key]]['consumers'].append(dict(rule=rid,atom=atom,slots=projection))
    return dict(schema=SCHEMA,recipe_sha256=recipe['sha256'],nodes=nodes,control=copy.deepcopy(recipe['control']),
        contract=dict(input='committed current row changes, coalesced before a legal consumer Run',
            result='complete correlated entry parameters and witness epoch',
            delivery='independent consumer watermarks; original RHS publication',
            invalidation='old/new witnesses, visibility, canonical identities and same-key forwarding',
            scope='table entry nodes; residual queries and guards remain exact'))
