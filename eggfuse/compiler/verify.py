"""Native-to-native full-state comparison after projecting only marker tables."""

def compare_source(sk,mk,marker_ir):
    names={m['name'] for m in marker_ir['markers']}|{s['receipt_table'] for s in marker_ir.get('continuations',[])}
    lhs=sk['rows'];rhs=[r for r in mk['rows'] if r['table'] not in names]
    def value(v):
        if 'elements' in v:return (v['sort'],frozenset(value(e) for e in v['elements']))
        return (v['sort'],v.get('eq',v.get('value')))
    forward={};inverse={}
    def pair(a,b):
        if forward.get(a,b)!=b or inverse.get(b,a)!=a:raise ValueError('different original equality partition')
        forward[a]=b;inverse[b]=a
    original={(a['sort'],a['old']):a['now'] for a in sk['anchors']}
    marked={(a['sort'],a['old']):a['now'] for a in mk['anchors']}
    if original.keys()!=marked.keys():raise ValueError('input identity anchors changed; comparison inconclusive')
    for (sort,old),now in marked.items():pair((sort,now),(sort,original[sort,old]))
    index={}
    for r in lhs:
        vs=tuple(map(value,r['values']));key=(r['table'],vs[:-1])
        if key in index:raise ValueError('duplicate canonical source key')
        index[key]=(vs[-1],r['subsumed'])
    if len(lhs)!=len(rhs):raise ValueError(f'original row count differs: {len(lhs)} vs {len(rhs)}')
    def mapped(v):
        if 'eq' in v:
            k=value(v)
            if k not in forward:raise KeyError(k)
            return forward[k]
        if 'elements' in v:return (v['sort'],frozenset(mapped(e) for e in v['elements']))
        return value(v)
    pending=rhs;checked=set()
    while pending:
        rest=[]
        for r in pending:
            try:args=tuple(mapped(v) for v in r['values'][:-1])
            except KeyError:rest.append(r);continue
            key=(r['table'],args)
            if key not in index:raise ValueError('missing original row '+str(key))
            target,hidden=index[key]
            if hidden!=r['subsumed']:raise ValueError('different original visibility')
            out=r['values'][-1]
            if 'eq' in out:pair(value(out),target)
            else:
                try:actual=mapped(out)
                except KeyError:rest.append(r);continue
                if actual!=target:raise ValueError('different original function value')
            if key in checked:raise ValueError('duplicate projected marked key')
            checked.add(key)
        if len(rest)==len(pending):raise ValueError('unanchored cycle: comparison inconclusive')
        pending=rest
    return dict(sk_equals_marked=True,original_rows=len(checked),original_eclasses=len(forward),projected_tables=sorted(names),
                scope='complete original facts, values, equivalence and visibility; marker maintenance is not original work')
