"""Bridge solver-generated shared-markers.egg to predictor inputs.

Consumes the existing @marker typed-prefix contract, including carried slots.
Does not choose new layouts, infer purity, or treat marker identity as Cell ID.
"""
from collections import defaultdict
import hashlib
import json


def load(path, schemas):
    text=path.read_text();definitions=[];seen=set()
    for line in text.splitlines():
        if not line.startswith('; @marker '):continue
        meta=json.loads(line[len('; @marker '):]);body=json.loads(meta['body_key'])
        atoms=body['atoms'];slots={};parts=[]
        for a in atoms:
            cols=a['columns'];sorts=[c['sort'] for c in cols]
            if a['table'] not in schemas or sorts!=schemas[a['table']]:
                raise ValueError('marker/native schema mismatch: '+a['table'])
            for c in cols:
                if set(c)!={'Slot','sort'}:
                    raise ValueError('summary feature adapter currently requires slot operands; no literal/guard may be erased')
                s=c['Slot']
                if s in slots and slots[s]!=c['sort']:raise ValueError('inconsistent slot type')
                slots[s]=c['sort']
            op=lambda c:'s'+str(c['Slot'])
            parts.append(f'(= {op(cols[-1])} ({a["table"]} {" ".join(op(c) for c in cols[:-1])}))')
        if sorted(slots)!=list(range(len(slots))):raise ValueError('noncanonical slots')
        carried=meta['carried'];name='SharedSpine'+str(meta['id'])
        if len(set(carried))!=len(carried) or not set(carried)<=slots.keys():raise ValueError('invalid carried slots')
        declaration=f'(relation {name} ({" ".join(slots[i] for i in carried)}))'
        rule=f'(rule ({" ".join(parts)}) (({name} {" ".join("s"+str(i) for i in carried)})) :ruleset cell-marker-definitions)'
        if declaration not in text.splitlines() or rule not in text.splitlines():
            raise ValueError('marker annotation differs from .egg definition')
        # Identity preserves complete joint wiring, not just operator names.
        key=json.dumps(atoms,sort_keys=True,separators=(',',':'))
        ident=hashlib.sha256(key.encode()).hexdigest()
        if (ident,tuple(carried)) in seen:continue
        seen.add((ident,tuple(carried)))
        definitions.append(dict(name=name,identity=ident,atoms=atoms,carried=carried,
            sorts=[slots[i] for i in range(len(slots))],dependencies=sorted({a['table'] for a in atoms})))
    if not definitions:raise ValueError('no solver marker contracts found')
    return definitions


def bindings(definition,tables):
    """Independent frozen-graph oracle, preserving joint assignments."""
    rows=[{}]
    for a in definition['atoms']:
        atom_rows=[]
        for row in tables.get(a['table'],[]):
            b={}
            for c,v in zip(a['columns'],row,strict=True):
                if v[1]!=c['sort']:raise ValueError('wrong row type')
                s=c['Slot']
                if s in b and b[s]!=v:break
                b[s]=v
            else:atom_rows.append(b)
        shared=sorted(set(rows[0]) & set(atom_rows[0])) if rows and atom_rows else []
        ix=defaultdict(list)
        for b in atom_rows:ix[tuple(b[s] for s in shared)].append(b)
        rows=[b | c for b in rows for c in ix[tuple(b[s] for s in shared)]]
    return {tuple(b[i] for i in range(len(definition['sorts']))) for b in rows}


def tokens(definitions,tables,model_sorts):
    result=defaultdict(set);vocab=[];evidence=[]
    for d in definitions:
        ports=[s for s in d['carried'] if d['sorts'][s] in model_sorts]
        ts={s:('summary',d['identity'],s) for s in ports}
        vocab.extend(ts.values());bs=bindings(d,tables)
        for b in bs:
            for s,t in ts.items():result[b[s]].add(t)
        evidence.append(dict(name=d['name'],atoms=len(d['atoms']),bindings=len(bs),ports=ports))
    return result,vocab,evidence


def excluded(definitions,vocab,channel):
    forbidden={d['identity'] for d in definitions if channel in d['dependencies']}
    return [i for i,t in enumerate(vocab) if t[0]=='summary' and t[1] in forbidden]
