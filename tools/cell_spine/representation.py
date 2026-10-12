"""Physical spine contracts, proved over every producer before solver selection.

Raw rows are immutable in the generated positive-constructor runtime. A finite
inductive invariant records which seed rows already exist in raw input storage.
This is row congruence, not constructor injectivity or a closure-existence guess.
"""
from . import takeover as t
from . import names
from .terms import text,walk


class Congruence:
    def __init__(self):self.terms={};self.parent={}
    def add(self,e):
        key=text(e)
        if key not in self.parent:
            self.parent[key]=key;self.terms[key]=e
            for a in e.get('args',[]):self.add(a)
        return key
    def find(self,k):
        while self.parent[k]!=k:
            self.parent[k]=self.parent[self.parent[k]];k=self.parent[k]
        return k
    def union(self,a,b):
        a,b=self.find(self.add(a)),self.find(self.add(b))
        if a==b:return False
        self.parent[b]=a;return True
    def close(self):
        while True:
            seen={};changed=False
            for e in list(self.terms.values()):
                if e['kind']!='call':continue
                key=self.row(e)
                if key in seen:changed=self.union(e,seen[key]) or changed
                else:seen[key]=e
            if not changed:return
    def row(self,e):return e['head'],tuple(self.find(self.add(a)) for a in e['args'])


def seed(c):return [t.fork(e,{p:t.var(v) for p,v in c['params'].items()}) for e in c['roots']]

def calls(roots):return {text(e):e for r in roots for e in walk(r) if e['kind']=='call'}

def summary(e):return names.is_summary(e.get('head',''))

def raw_contracts(chosen,rules):
    """Greatest inductive row invariant, checked against ALL rule producers.

    Start with all seed rows; remove a row when any producer cannot preserve
    it. Cycles are sound by induction from actual ingress, not by assuming a
    cyclic family exists. A family with no producer gets no borrowed rows.
    """
    roots={f:seed(c) for f,c in chosen.items()}
    terms={f:calls(rs) for f,rs in roots.items()}
    guaranteed={f:set(ts) for f,ts in terms.items()}
    proofs=[]
    for rule in rules:
        targets=[a['expr'] for a in rule['head'] if a['kind']=='expr' and summary(a['expr'])]
        if not targets:continue
        proof=Congruence();external=[];sources=[];identities=[]
        for fact in rule['body']:
            es=fact['args'] if fact['kind']=='eq' else [fact['expr']]
            for e in es:proof.add(e)
            if fact['kind']=='eq':proof.union(*es)
            if fact['kind']=='fact' and summary(es[0]):
                e=es[0];f=names.family(e['head'])
                fields=[f'r{i}' for i in range(len(roots[f]))]+list(chosen[f]['params'].values())
                subst=dict(zip(fields,e['args']))
                mapped={k:t.replace(v,subst) for k,v in terms[f].items()}
                for i,r in enumerate(roots[f]):
                    bound=t.replace(r,subst);proof.union(e['args'][i],bound);identities.append(bound)
                sources.append((f,mapped))
            else:external.extend(calls(es).values())
        mapped_targets=[]
        for e in targets:
            f=names.family(e['head'])
            fields=[f'r{i}' for i in range(len(roots[f]))]+list(chosen[f]['params'].values())
            subst=dict(zip(fields,e['args']))
            mapped={k:t.replace(v,subst) for k,v in terms[f].items()}
            for v in mapped.values():proof.add(v)
            for r in e['args'][:len(roots[f])]:proof.add(r)
            mapped_targets.append((f,mapped,e['args'][:len(roots[f])]))
        proof.close()
        owned={proof.row(e) for _,ts in sources for e in ts.values()}
        # A seed row may be substituted by its marker value in the lowered
        # LHS. Do not count that removed query as evidence of a raw table row.
        ext={proof.row(e) for e in external}-owned
        proofs.append((proof,ext,sources,mapped_targets,identities))
    while True:
        next_guaranteed={f:set(ts) for f,ts in guaranteed.items()};produced=set()
        for proof,external,sources,targets,identities in proofs:
            available=external|{proof.row(ts[k]) for f,ts in sources for k in guaranteed[f]}
            for f,ts,root_args in targets:
                produced.add(f)
                next_guaranteed[f]&={k for k,e in ts.items() if proof.row(e) in available}
                # Omitting the canonical root must not create a fresh identity
                # by querying a raw constructor that never existed.
                if any(r['kind']=='call' and proof.row(r) not in available and
                       not any(text(r)==text(v) for v in identities) for r in root_args):
                    next_guaranteed[f]=set()
        for f in next_guaranteed:
            if f not in produced:next_guaranteed[f]=set()
        if next_guaranteed==guaranteed:return guaranteed
        guaranteed=next_guaranteed


def extend_layouts(bundle):
    """Add full borrowed forests and single-root boundary representations.

    Boundary form carries outside inputs, decodes one immediate subtree, and
    omits the already existing raw root. Multi-root and union-guard forests
    retain full decoding; no input correlations are erased.
    """
    raw=raw_contracts(bundle['chosen'],bundle['consumers']);chosen={}
    for f,c in bundle['chosen'].items():
        rs=seed(c);all_terms=set(calls(rs));ls=list(c['marker_layouts']);seen=set()
        def add(base,mode,decode,stored,read):
            key=(tuple(map(text,base['terms'])),tuple(t.fact_text(q) for q in decode),tuple(map(text,stored)),read)
            if key in seen:return
            seen.add(key)
            ls.append(dict(base,id=f'{base["id"]}_{mode}',representation=mode,decode=decode,
                stored_roots=stored,read_storage=read,
                scope='inductive raw-row contract; all producers share this marker ABI'))
        for l in c['marker_layouts']:
            if all_terms<=raw[f]:add(l,'borrowed',l['decode'],[], 'raw')
            if len(rs)!=1 or any(a['kind']=='union' for a in c['acts']):continue
            anchor=l['anchor']
            if anchor['kind']!='call' or anchor not in rs[0].get('args',[]):continue
            # Exactly one subtree is decoded. All other branches must be
            # exposed inputs/literals; arbitrary forests are not guessed.
            if any(a['kind']=='call' and a!=anchor for a in rs[0]['args']):continue
            if text(rs[0]) not in raw[f]:continue
            present={v['name'] for v in l['terms'] if v['kind']=='var'}
            inside={v['name'] for v in walk(anchor) if v['kind']=='var'}
            if set(c['params'].values())-inside-present:continue
            query=[dict(kind='eq',args=[t.var('anchor'),anchor])] if inside-present else []
            add(l,'boundary',query,[anchor],'canonical')
            if set(calls([anchor]))<=raw[f]:add(l,'boundary_borrowed',query,[],'raw')
        chosen[f]=dict(c,marker_layouts=ls)
    return dict(bundle,chosen=chosen,raw_contracts={str(f):sorted(v) for f,v in raw.items()})
