"""Incremental typed wiring compiler. Truth stays resident on the GPU.

Input: structural row upserts/removals, authoritative typed aliases, and explicit
external predicate seeds. No closure output or complete query answer is needed.
"""
import copy
import struct
from collections import defaultdict
from compiler import Templates, separator

class DynamicContext:
    def __init__(self,core,ruleset):
        self.templates=Templates(core,ruleset);self.epoch=0
        self.outputs=sorted(self.templates.outputs)
        self.rows={};self.watch=defaultdict(set);self.parents={}
        self.value_facts=defaultdict(dict);self.coordinates=[];self.fact_parent=[]
        self.owner_clauses={};self.supports={};self.banks=defaultdict(list);self.locations={}

    def library(self):
        return {'schema':'matrix-dynamic/v1','rule_sha256':self.templates.digest,
                'predicates':self.outputs,'separators':[separator(k) for k in range(self.templates.max_arity+1)]}

    def root(self,key):
        trail=[]
        while key in self.parents:trail.append(key);key=self.parents[key]
        for old in trail:self.parents[old]=key
        return key

    def fact_root(self,i):
        while self.fact_parent[i]!=i:
            self.fact_parent[i]=self.fact_parent[self.fact_parent[i]];i=self.fact_parent[i]
        return i

    def canonical(self,value):
        if 'eq' not in value:return value
        sort,name=self.root((value['sort'],value['eq']))
        return {'sort':sort,'eq':name}

    def coordinate(self,predicate,value):
        value=self.canonical(value);key=(value['sort'],value['eq'])
        known=self.value_facts[key]
        if predicate not in known:
            known[predicate]=len(self.coordinates);self.fact_parent.append(len(self.coordinates))
            self.coordinates.append({'table':predicate,'input':value})
        return self.fact_root(known[predicate])

    def row_values(self,row):
        return {self.root((v['sort'],v['eq'])) for v in row['values'] if 'eq' in v}

    def unwatch(self,owner,row):
        for value in self.row_values(row):
            subscribers=self.watch.get(value)
            if subscribers is not None:
                subscribers.discard(owner)
                if not subscribers:self.watch.pop(value,None)

    def apply(self,*,upserts=(),removes=(),aliases=(),seeds=(),expected_epoch=None):
        if expected_epoch is not None and expected_epoch!=self.epoch:raise ValueError('stale event epoch')
        if self.epoch==0xffffffff:raise ValueError('epoch exhausted')
        # Reject malformed batches before touching the registry. All variable
        # grounding obligations were checked when Templates was constructed.
        for owner,row in upserts:self.templates.validate_row(row)
        for sort,a,b in aliases:
            if not all(isinstance(x,str) for x in [sort,a,b]):raise ValueError('typed alias')
        for predicate,value,hidden in seeds:
            if predicate not in self.outputs or value.get('sort')!=self.templates.tables[predicate]['schema'][0]['name'] or not isinstance(value.get('eq'),str) or hidden not in [False,True]:raise ValueError('typed external seed')
        old_epoch=self.epoch;self.epoch+=1;epoch=self.epoch;old_facts=len(self.coordinates)
        affected=set();merges=[]
        for sort,a,b in aliases:
            if sort not in self.templates.alias_sorts:continue
            old=self.root((sort,a));new=self.root((sort,b))
            if old==new:continue
            affected.update(self.watch.get(old,()));affected.update(self.watch.get(new,()))
            self.parents[old]=new
            self.watch[new].update(self.watch.pop(old,set()))
            old_map=self.value_facts.pop(old,{})
            new_map=self.value_facts[new]
            for predicate,slot in old_map.items():
                slot=self.fact_root(slot)
                if predicate in new_map:
                    other=self.fact_root(new_map[predicate])
                    if slot!=other:
                        lo,hi=sorted([slot,other]);self.fact_parent[hi]=lo;merges.append((hi,lo));slot=lo
                new_map[predicate]=slot
        for owner in removes:
            previous=self.rows.pop(owner,None)
            if previous:
                affected.add(owner)
                self.unwatch(owner,previous)
        for owner,row in upserts:
            if row['table'] not in self.templates.by_table:continue
            previous=self.rows.get(owner)
            if previous:
                self.unwatch(owner,previous)
            self.rows[owner]=copy.deepcopy(row);affected.add(owner)
            for value in self.row_values(row):self.watch[value].add(owner)
        touched=set();was_alive=set()
        for owner in sorted(affected,key=str):
            old=self.owner_clauses.pop(owner,set());row=self.rows.get(owner)
            if row:
                current=dict(row,values=[self.canonical(v) for v in row['values']])
                new=self.templates.ground(current,self.coordinate)
                self.owner_clauses[owner]=new
            else:new=set()
            for key in old|new:
                if key not in touched and self.supports.get(key):was_alive.add(key)
                touched.add(key)
            for key in old-new:self.supports[key].discard(owner)
            for key in new-old:self.supports.setdefault(key,set()).add(owner)
        retired=sorted(k for k in touched if k in was_alive and not self.supports.get(k))
        added=sorted(k for k in touched if k not in was_alive and self.supports.get(k))
        changes=defaultdict(dict);old_counts={k:len(v) for k,v in self.banks.items()}
        # Swap removal makes banks dense immediately. Moved/recycled slots get
        # this epoch as their generation, so no per-history generation array grows.
        for key in retired:
            k,slot=self.locations.pop(key);bank=self.banks[k];last=bank.pop()
            self.supports.pop(key,None)
            if slot<len(bank):
                bank[slot]=last;self.locations[last]=(k,slot);changes[k][slot]=last
            changes[k].pop(len(bank),None)
        for key in added:
            k=len(key[0]);slot=len(self.banks[k]);self.banks[k].append(key)
            self.locations[key]=(k,slot);changes[k][slot]=key
        seed_rows=[]
        for predicate,value,hidden in seeds:seed_rows.append((self.coordinate(predicate,value),1,int(hidden)))
        words=[1,old_epoch,epoch,len(self.coordinates),len(self.coordinates)-old_facts,len(merges),len(seed_rows),0]
        words += [self.outputs.index(c['table']) for c in self.coordinates[old_facts:]]
        words += [x for pair in merges for x in pair]
        words += [x for row in seed_rows for x in row]
        banks=[]
        for k in sorted(set(old_counts)|set(self.banks)):
            before=old_counts.get(k,0);after=len(self.banks[k]);updates=changes.get(k,{})
            if before==after and not updates:continue
            banks.append(k);words += [k,before,after,len(updates)]
            for slot,(deps,target) in sorted(updates.items()):words += [slot,epoch,target,*deps]
        words[7]=len(banks)
        if any(not 0<=v<=0xffffffff for v in words):raise ValueError('device coordinate range')
        if old_epoch and len(self.coordinates)==old_facts and not merges and not seed_rows and not banks:
            # Unrelated native changes or replacement by an identical supported
            # clause do not invalidate this interface's GPU version.
            self.epoch=old_epoch;epoch=old_epoch;words=None
        return {'words':words,'stats':{'epoch':epoch,'anchors_regrounded':len(affected),
                 'contexts_retired':len(retired),'contexts_added':len(added),
                 'live_contexts':sum(map(len,self.banks.values())),'new_fact_slots':len(self.coordinates)-old_facts,
                 'fact_merges':len(merges),'updated_banks':len(banks)}}

    def bootstrap(self,rows):
        structural=[(i,r) for i,r in enumerate(rows) if r['table'] in self.templates.by_table]
        seeds=[(r['table'],r['values'][0],r['subsumed']) for r in rows if r['table'] in self.outputs]
        return self.apply(upserts=structural,seeds=seeds)

    def decode(self,state,hidden,parents):
        result={name:set() for name in self.outputs}
        for i,c in enumerate(self.coordinates):
            root=i
            while parents[root]!=root:root=parents[root]
            if root!=self.fact_root(i):raise AssertionError('GPU fact alias partition differs')
            if state[root] and not hidden[root]:result[c['table']].add(self.canonical(c['input'])['eq'])
        return {k:sorted(v) for k,v in result.items()}

def pack(words):return struct.pack('<I',len(words)*4)+struct.pack('<'+'I'*len(words),*words)
