"""Persistent source-table Spine prefixes and correlated application receipts.

These are ordinary Unit-valued functions. Mutable ports remain authoritative
in the original tables; a historical receipt is never a current-value cache.
"""
import copy
import json
from .terms import walk
from . import takeover as t


def prefix_states(selected):
    """Intern proper prefixes, preserving the selected views' field numbering."""
    result=list(selected); by_body={json.dumps(v['body'],sort_keys=True):v for v in selected}
    def parent(v):
        if 'completion_body' in v:return
        v['state_kind']='structural-prefix';v['parent']=None
        if len(v['body'])<=2:
            v['completion_body']=copy.deepcopy(v['body']);return
        body=v['body'][:-1];key=json.dumps(body,sort_keys=True);p=by_body.get(key)
        if p is None:
            names={e['name'] for f in body for x in f['args'] for e in walk(x) if e['kind']=='var'}
            fields=[n for n in v['parameters'] if n in names]
            # canonical() numbers coordinates in first-observation order.
            assert fields==v['parameters'][:len(fields)]
            p=dict(name=f'CellSpineView{len(result)}',body=copy.deepcopy(body),
                   sorts=v['sorts'][:len(fields)],parameters=fields,occurrences=[],
                   cells=v['cells'],evidence=v['evidence'],support_for=[])
            result.append(p);by_body[key]=p
        p.setdefault('support_for',[]).append(v['name']);parent(p)
        v['parent']=p['name']
        v['completion_body']=[dict(kind='fact',expr=t.call(p['name'],[t.var(n) for n in p['parameters']])),copy.deepcopy(v['body'][-1])]
    for v in selected:parent(v)
    return result


def applied_states(original,flats,by_rule,sig,changed):
    result={}
    for rid,uses in by_rule.items():
        r=flats[rid];types=dict(r['types'])
        def infer(e,expected=None):
            if e['kind']=='var':
                if expected:types[e['name']]=expected
                return types.get(e['name'])
            if e['kind']=='literal':return e['sort']
            s=sig.get(e['head'])
            if s:
                for a,sort in zip(e['args'],s['input']):infer(a,sort)
                return s['output']
            return None
        for f in r['body']:
            if f['kind']=='eq':
                a,b=f['args'];sa,sb=infer(a),infer(b)
                if sa:infer(b,sa)
                if sb:infer(a,sb)
            else:infer(f['expr'])
        bound={v['name'] for f in r['body'] for e in (f['args'] if f['kind']=='eq' else [f['expr']]) for v in walk(e) if v['kind']=='var'}
        inputs=set()
        for action in r['head']:
            roots=([action['expr']] if action['kind'] in ('expr','let') else action.get('args',[])+([action['value']] if action['kind']=='set' else []))
            inputs.update(v['name'] for e in roots for v in walk(e) if v['kind']=='var')
        # The Spine keeps the complete structural tuple. An effect receipt
        # needs only the input projection that determines its effects.
        variables=sorted(bound & inputs)
        # Unresolved primitive/global types are not guessed. Their consumers
        # still receive persistent structural prefixes and exact port queries.
        if any(v not in types for v in variables):continue
        table=f'CellSpineApplied{rid}'
        safe=True
        for a in r['head']:
            if a['kind'] not in ('expr','let'):safe=False;continue
            for e in walk(a['expr']):
                if e['kind']=='call' and (sig.get(e['head'],{}).get('kind')!='constructor' or e['head'] in changed):safe=False
        result[rid]=dict(table=table,rule=original[rid]['name'],ruleset=original[rid]['ruleset'],
            parameters=variables,sorts=[types[v] for v in variables],
            markers=[v['name'] for v,_ in uses],
            replay='immutable-constructor-effects' if safe else 'historical-observation-only',
            action=dict(kind='set',head=table,args=[t.var(v) for v in variables],value=dict(kind='literal',sort='Unit',text='()')))
    return result
