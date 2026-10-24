"""Whole-query-set context inventory. No rule/sort names are privileged.

Nodes are typed, correlated prefixes in the native plan domain. This inventory
exposes the domain to the joint selector; it is not a saturation certificate.
"""
from . import shared_prefix


def discover(queries):
    contexts={}
    for qi,q in enumerate(queries):
        for p in q['plans']:
            parent=None
            for depth in range(len(p['steps'])+1):
                d=shared_prefix.describe(dict(p,prefix_len=depth,carried_slots=[]))
                key=d['body_key']
                c=contexts.setdefault(key,dict(body=d['body'],parent=parent,uses=[]))
                assert c['parent']==parent
                use=dict(query=qi,seed=p['seed'],prefix_len=depth,slots=d['slots'])
                if use not in c['uses']:c['uses'].append(use)
                parent=key
    ids={key:i for i,key in enumerate(sorted(contexts))}
    nodes=[dict(id=ids[key],body=c['body'],parent=ids[c['parent']] if c['parent'] is not None else None,
        uses=c['uses'],consumer_count=len({u['query'] for u in c['uses']})) for key,c in sorted(contexts.items())]
    return dict(schema='query-context-domain/v1',queries=[dict(ruleset=q['ruleset'],rule=q['rule']) for q in queries],nodes=nodes,
        summary=dict(queries=len(queries),contexts=len(nodes),cross_consumer_contexts=sum(c['consumer_count']>1 for c in nodes),
            multi_atom_cross_consumer_contexts=sum(c['consumer_count']>1 and len(c['body']['atoms'])>1 for c in nodes)),
        scope='all native-admitted roots and fixed-order prefixes, typed aliases/literals/slot maps retained; not arbitrary join orders or semantic Cell equivalence')
