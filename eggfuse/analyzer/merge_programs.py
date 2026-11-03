"""Compile ordered merge expressions and their scheduled consumers.

This is an execution contract, not permission to substitute a terminal Cell.
No rule/table names, observed values, CSE of reads, or winner-only summaries.
"""
import copy
from eggfuse.ir.recipe import require

SCHEMA='eggfuse-merge-programs/v1'

def build(recipe):
    tables={t['name']:t for t in recipe['tables']}; programs=[]; constructed=set()
    types={s['name']:s for t in tables.values() for s in t['schema']}
    for r in recipe['rules']:
        for s in r['slots']:types[s['sort']['name']]=s['sort']
    for name in ('i64','bool','Unit','String','f64'):types.setdefault(name,dict(name=name,kind=name))
    for table in recipe['tables']:
        expr=table.get('merge_expr')
        if expr is None or table['kind']=='constructor':continue
        sorts=[table['schema'][-1],table['schema'][-1]];ops=[]
        def lower(e):
            if 'var' in e:
                require(e['var'] in ('old','new'),'unknown merge parameter')
                i=0 if e['var']=='old' else 1
                require(e.get('sort',sorts[i])==sorts[i],'merge parameter type differs')
                return {'slot':i}
            if 'literal' in e:return copy.deepcopy(e)
            args=[lower(a) for a in e['args']];call=e['call']
            if 'table' in call:
                t=tables[call['table']];expected=t['schema'][:-1];out=t['schema'][-1]
                kind='construct' if t['kind']=='constructor' or (out['kind']=='Unit' and t.get('merge_expr') is None) else 'read'
                if kind=='construct':constructed.add(t['name'])
                require(len(expected)==len(args),'merge call arity')
                require(all((sorts[a['slot']] if 'slot' in a else a['sort'])==s for a,s in zip(args,expected)),'merge argument type')
            else:
                out=types[call['output']];kind='compute'
                require([ (sorts[a['slot']] if 'slot' in a else a['sort'])['name'] for a in args]==call['inputs'],'merge primitive types')
                require(out['name']==call['output'],'merge primitive output')
            require(e.get('sort',out)==out,'merge output type')
            dst=len(sorts);sorts.append(copy.deepcopy(out))
            ops.append(dict(dst=dst,call=copy.deepcopy(call),args=args,kind=kind))
            return {'slot':dst}
        result=lower(expr)
        require((sorts[result['slot']] if 'slot' in result else result['sort'])==table['schema'][-1],'merge result type')
        programs.append(dict(table=table['name'],slots=sorts,operations=ops,result=result,
            equal_inputs='return_old' if 'table' in expr.get('call',{}) else 'evaluate'))
    # Follow exact row projections (including source-level marker publishers).
    # These relay existing slots; no name convention or inferred invariant.
    relays=[]
    while True:
        fresh=False
        for rid,r in enumerate(recipe['rules']):
            if not any(a['call'].get('table') in constructed for a in r['query']):continue
            targets=[]
            for e in r['effects']:
                if e['op']=='copy':continue
                t=tables.get(e.get('call',{}).get('table'))
                if e['op']!='set' or t is None or t['kind']!='function' or t['schema'][-1]['kind']!='Unit' or t.get('merge_expr') not in (None,{'var':'old'}):break
                if e['value'].get('sort',{}).get('kind')!='Unit' or 'literal' not in e['value']:break
                targets.append(t['name'])
            else:
                for name in targets:
                    if name not in constructed:
                        constructed.add(name);fresh=True;relays.append(dict(rule=rid,table=name))
        if not fresh:break
    entries=[];unsupported=[]
    for rid,r in enumerate(recipe['rules']):
        for anchor,a in enumerate(r['query']):
            if a['call'].get('table') not in constructed:continue
            known={v['slot'] for v in a['args'] if 'slot' in v};remaining=set(range(len(r['query'])))-{anchor};order=[]
            while remaining:
                ready=[]
                for i in sorted(remaining):
                    q=r['query'][i];full=all('slot' not in v or v['slot'] in known for v in q['args'][:-1])
                    t=tables.get(q['call'].get('table'));out=q['args'][-1]
                    inverse=t is not None and t['kind']=='constructor' and t['schema'][-1]['kind']=='eq' and ('slot' not in out or out['slot'] in known)
                    if full or inverse:ready.append((0 if 'primitive' in q['call'] else (1 if full else 2),i))
                if not ready:break
                _,i=min(ready);order.append(i);remaining.remove(i)
                known.update(v['slot'] for v in r['query'][i]['args'] if 'slot' in v)
            if remaining:
                unsupported.append(dict(rule=rid,anchor=anchor,residual=sorted(remaining)))
            else:entries.append(dict(rule=rid,anchor=anchor,atoms=order))
    return dict(schema=SCHEMA,recipe_sha256=recipe['sha256'],programs=programs,entries=entries,unsupported=unsupported,relays=relays,
        control=copy.deepcopy(recipe['control']),
        contract=dict(old_value='read by the existing ordered set/repair commit',
            operands='typed local slots; exact integers; full constructor alternatives',
            publication='original Run StagePatch and repair boundaries',
            invalidation='read current canonical visible ports; late rows use original delta entries',
            effects='original complete RHS; no winner-only reduction',
            terminal_cell_substitution=False))
