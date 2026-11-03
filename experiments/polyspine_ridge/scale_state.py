"""Lossless compact snapshots, union lineage and observable answer changes."""
from collections import defaultdict
import hashlib
import json
from features import identity, STAGES, View


class Lineage:
    def __init__(self):self.parent={}
    def find(self,k):
        path=[]
        while k in self.parent and self.parent[k]!=k:
            path.append(k);k=self.parent[k]
        for p in path:self.parent[p]=k
        return k
    def apply(self,aliases):
        for a in aliases:
            old=(a['sort'],'eq',a['old']);new=(a['sort'],'eq',a['now'])
            old=self.find(old);new=self.find(new)
            if old!=new:self.parent[old]=new
    def normalize(self,x):
        if isinstance(x,tuple):
            if len(x)==3 and x[1]=='eq':return self.find(x)
            return tuple(self.normalize(v) for v in x)
        return x


class DeltaState:
    def __init__(self):self.tables=None;self.values=[];self.rows=set();self.lineage=Lineage();self.bytes=0
    def apply(self,p):
        if self.tables is None:self.tables=p['tables']
        if self.tables!=p['tables']:raise ValueError('table dictionary changed')
        for index,v in p['values']:
            if index!=len(self.values):raise ValueError('value dictionary discontinuity')
            self.values.append(v)
        for r in p['remove']:
            r=tuple(r)
            if r not in self.rows:raise ValueError('removing unknown row')
            self.rows.remove(r)
        for r in p['insert']:
            r=tuple(r)
            if r in self.rows:raise ValueError('duplicate insertion')
            if any(i>=len(self.values) for i in r[2:]):raise ValueError('unknown value')
            self.rows.add(r)
        if len(self.rows)!=p['rows']:raise ValueError('delta row count mismatch')
        self.lineage.apply(p['aliases']);self.bytes+=len(json.dumps(p,separators=(',',':')))
    def snapshot(self):
        return {'rows':[{'table':self.tables[r[0]],'subsumed':bool(r[1]),
                         'values':[self.values[i] for i in r[2:]]} for r in sorted(self.rows)],'anchors':[]}


def selected_tables(core):
    v=View(core);tables=v.tables
    ordinary={n for n in v.ordinary if any(s['name']=='Expr' for s in tables[n]['schema'])}
    interfaces=set().union(*(v.readers[s] for members,_ in STAGES.values() for s in members))
    interfaces={n for n in interfaces if any(s['name']=='Expr' for s in tables[n]['schema'])}
    wanted=ordinary|interfaces|{target for _,target in STAGES.values()}
    # All head alternatives for values used by these rows; do not recursively
    # export the entire Term graph. TCPair's primitive costs are included.
    sorts={s['name'] for n in wanted for s in tables[n]['schema']}
    wanted|={n for n in v.ordinary if tables[n]['schema'][-1]['name'] in sorts}
    from residual import compile_recipes
    programs={s:compile_recipes(core,t,s) for s,t in [('always-run','ExprIsPure'),('is-resolved','ExprIsResolved')]}
    for p in programs.values():wanted.update(p['family']);wanted.update(r['anchor'] for r in p['recipes'])
    return sorted(wanted),programs


def answers(snapshot,core,target):
    tables={t['name']:t for t in core['tables']};meta=tables[target]
    relation=meta['schema'][-1]['name'].startswith('___')
    rows=[r for r in snapshot['rows'] if not r['subsumed']]
    children=defaultdict(set)
    if meta['kind']=='function' and meta['schema'][-1]['kind']=='eq':
        ret=meta['schema'][-1]['name']
        for r in rows:
            t=tables[r['table']]
            if t['kind']=='constructor' and t['schema'][-1]['name']==ret:
                children[identity(r['values'][-1])].add((r['table'],tuple(map(identity,r['values'][:-1]))))
    out=defaultdict(set)
    for r in rows:
        if r['table']!=target:continue
        vals=list(map(identity,r['values']));payload=tuple(vals[1:-1] if relation else vals[1:])
        if children and not relation:payload+= (('result-view',tuple(sorted(children[vals[-1]]))),)
        out[vals[0]].add(payload)
    return out


def changes(keys,before,after,lineage):
    gained=[];lost=[];present=[];sizes=[];events=[]
    for key in keys:
        old={lineage.normalize(x) for x in before.get(key,())};new=after.get(lineage.find(key),set())
        gained.append(bool(new-old));lost.append(bool(old-new));present.append(bool(new));sizes.append(len(before.get(key,())))
        events.append(hashlib.blake2b(repr((sorted(old),sorted(new))).encode(),digest_size=8).digest())
    return gained,lost,present,sizes,events


def holdout_group(key):
    # Identity determines the split only, never a regression feature. All
    # historical roots unified by native union must use this same final key.
    return int.from_bytes(hashlib.blake2b(repr(('polyspine-scale-v1',key)).encode(),digest_size=8).digest(),'little')%5==0
