"""Compile anchor-complete, guard-free queries into shared response matrices.

One anchor row binds EVERY variable shared between atoms. Other atoms then
have independent existential witnesses conditioned on that complete binding.
Their rows remain payloads: sharing a response never unions term identities.
"""
from collections import Counter,defaultdict
import numpy as np


def variables(atom):return {c['Slot'] for c in atom['columns'] if 'Slot' in c}


def choose_anchor(shape):
    if shape.get('guards') or shape.get('native_residuals'):return None
    counts=Counter(v for a in shape['atoms'] for v in variables(a))
    shared={v for v,n in counts.items() if n>1}
    choices=[i for i,a in enumerate(shape['atoms']) if shared<=variables(a)]
    return max(choices,key=lambda i:(len(shape['atoms'][i]['columns']),-i)) if choices else None


def local_key(atom):
    ids={};cols=[]
    for c in atom['columns']:
        if 'Literal' in c:cols.append(('literal',c['Literal']))
        else:
            slot=c['Slot'];ids.setdefault(slot,len(ids));cols.append(('slot',ids[slot]))
    return (atom['table'],tuple(atom['sorts']),tuple(cols)),ids


def row_binding(atom,row):
    binding={}
    for col,sort,v in zip(atom['columns'],atom['sorts'],row,strict=True):
        if v[1]!=sort:raise ValueError('typed table mismatch')
        if 'Literal' in col:
            if v!=('Base',sort,col['Literal']):return None
        else:
            slot=col['Slot']
            if slot in binding and binding[slot]!=v:return None
            binding[slot]=v
    return binding


def projection_index(key,rows):
    table,sorts,columns,kept=key;out=defaultdict(list)
    for rid,row in enumerate(rows):
        binding={};valid=True
        for (kind,arg),sort,v in zip(columns,sorts,row,strict=True):
            if v[1]!=sort:raise ValueError(f'sort mismatch: {table}')
            if kind=='literal':
                if v!=('Base',sort,arg):valid=False;break
            elif arg in binding and binding[arg]!=v:valid=False;break
            else:binding[arg]=v
        if valid:out[tuple(binding[s] for s in kept)].append(rid)
    return out


def fold_matrix(z,w,threshold):
    """Exact cofactor under checked constant/equal-column assumptions.

    These assumptions describe this feature bank. A new context must check
    them or use the original W; observed equality is not a timeless theorem.
    Coalesced columns give non-Boolean integer weights without new arithmetic.
    """
    constants={};equal=[];classes={};representatives=[]
    t=threshold.astype(np.int32).copy();weights=[]
    for c in range(z.shape[1]):
        column=z[:,c]
        if len(column) and np.all(column==column[0]):
            constants[c]=int(column[0]);t-=int(column[0])*w[c];continue
        sig=column.tobytes()
        if sig in classes:
            k=classes[sig];weights[k]+=w[c].astype(np.int32);equal.append((c,representatives[k]))
        else:
            classes[sig]=len(weights);weights.append(w[c].astype(np.int32));representatives.append(c)
    folded=np.array(weights,dtype=np.int32).reshape(len(weights),w.shape[1])
    # Several different rules can have the same predicate response, while their
    # original atoms and RHS payload maps remain separate.
    columns=np.concatenate([folded.T,t[:,None]],axis=1)
    unique,route=np.unique(columns,axis=0,return_inverse=True)
    return {'z':z[:,representatives],'w':unique[:,:-1].T,'threshold':unique[:,-1],
        'gate_route':route,'representatives':representatives,'constants':constants,
        'equal_columns':equal}


def evaluate_folded_or_original(x,w,threshold,fold):
    valid=all(np.all(x[:,c]==v) for c,v in fold['constants'].items())
    valid=valid and all(np.array_equal(x[:,a],x[:,b]) for a,b in fold['equal_columns'])
    if not valid:return x.astype(np.int32)@w.astype(np.int32)==threshold,False
    values=x[:,fold['representatives']].astype(np.int32)@fold['w']==fold['threshold']
    return values[:,fold['gate_route']],True


class ResponseBank:
    def __init__(self,queries,tables):
        self.tables=tables;self.queries=queries;self.groups={};self.indices={}
        self.eligible={};self.stats=Counter()
        for qi,q in enumerate(queries):
            shape=q['shape'];anchor=choose_anchor(shape)
            if anchor is None:continue
            self.eligible[qi]=anchor
            a=shape['atoms'][anchor];name=a['table']
            group=self.groups.setdefault(name,{'rules':[],'features':{},'requirements':[]})
            anchor_positions={}
            for i,c in enumerate(a['columns']):
                if 'Slot' in c:anchor_positions.setdefault(c['Slot'],i)
            # The anchor's own literals and repeated-variable equalities.
            ak,_=local_key(a);feature=('anchor',ak)
            group['features'].setdefault(feature,len(group['features']))
            needed={group['features'][feature]}
            for i,atom in enumerate(shape['atoms']):
                if i==anchor:continue
                (table,sorts,cols),ids=local_key(atom)
                bound=[s for s in ids if s in anchor_positions]
                kept=tuple(ids[s] for s in bound)
                key=(table,sorts,cols,kept)
                feature=('lookup',key,tuple(anchor_positions[s] for s in bound))
                group['features'].setdefault(feature,len(group['features']))
                needed.add(group['features'][feature])
                if key not in self.indices:self.indices[key]=projection_index(key,tables.get(table,[]))
            group['rules'].append(qi);group['requirements'].append(needed)

    def encode(self):
        for name,g in self.groups.items():
            rows=self.tables.get(name,[]);nf=len(g['features']);nq=len(g['rules'])
            x=np.zeros((len(rows),nf),dtype=np.uint8)
            for feature,col in g['features'].items():
                if feature[0]=='anchor':
                    _,(_,sorts,columns)=feature
                    atom={'columns':[{'Slot':v} if k=='slot' else {'Literal':v} for k,v in columns],'sorts':sorts}
                    x[:,col]=[row_binding(atom,row) is not None for row in rows]
                    self.stats['anchor_row_checks']+=len(rows)
                else:
                    _,key,positions=feature;idx=self.indices[key];memo={}
                    for r,row in enumerate(rows):
                        projected=tuple(row[p] for p in positions)
                        if projected not in memo:memo[projected]=bool(idx.get(projected));self.stats['distinct_membership_reads']+=1
                        x[r,col]=memo[projected]
                    self.stats['context_feature_occurrences']+=len(rows)
            w=np.zeros((nf,nq),dtype=np.uint8)
            for q,need in enumerate(g['requirements']):w[list(need),q]=1
            # Exact quotient of observed Boolean contexts, not a sampled hash.
            # Routing retains each original row and all original payloads.
            profiles,inverse=np.unique(x,axis=0,return_inverse=True)
            threshold=w.sum(0,dtype=np.int32)
            response=profiles.astype(np.int32)@w.astype(np.int32)==threshold
            folded=fold_matrix(profiles,w,threshold)
            reconstructed,valid=evaluate_folded_or_original(profiles,w,threshold,folded)
            assert valid and np.array_equal(reconstructed,response)
            g.update({'x':x,'w':w,'profiles':profiles,'route':inverse,
                      'threshold':threshold,'response':response,
                      'answer':response[inverse],'fold':folded})
        return self

    def answers(self):
        return {(qi,r) for g in self.groups.values() for j,qi in enumerate(g['rules'])
                for r in np.flatnonzero(g['answer'][:,j])}

    def binding_factors(self,qi,rid):
        """Lazy full binding representation: anchor + independent whole-row sets.

        Enumerating the product recovers bindings; no column marginals are
        mixed. Primitive guards were excluded, not erased. Duplicate complete
        bindings may later be deduplicated just as a native projected query.
        """
        shape=self.queries[qi]['shape'];ai=self.eligible[qi];a=shape['atoms'][ai]
        anchor=row_binding(a,self.tables[a['table']][rid])
        if anchor is None:return None
        payload=[]
        for i,atom in enumerate(shape['atoms']):
            if i==ai:continue
            (table,sorts,cols),ids=local_key(atom)
            bound=[s for s in ids if s in anchor]
            key=(table,sorts,cols,tuple(ids[s] for s in bound))
            witnesses=self.indices[key].get(tuple(anchor[s] for s in bound),[])
            if not witnesses:return None
            payload.append((i,witnesses))
        return anchor,payload

    def summary(self):
        groups=[]
        for name,g in self.groups.items():
            n,f=g['x'].shape;q=len(g['rules']);p=len(g['profiles'])
            groups.append({'table':name,'rules':q,'rule_ids':g['rules'],'rows':n,
                'features':f,'distinct_context_profiles':p,'positive_root_rule_pairs':int(g['answer'].sum()),
                'raw_score_outputs':n*q,'shared_score_outputs':p*q,
                'raw_gemm_flops':2*n*f*q,'shared_gemm_flops':2*p*f*q,
                'cofactor_features':g['fold']['z'].shape[1],
                'cofactor_gates':len(g['fold']['threshold']),
                'cofactor_gemm_flops':2*p*g['fold']['z'].shape[1]*len(g['fold']['threshold']),
                'distinct_rule_response_masks':len(np.unique(g['response'],axis=0)),
                'feature_density':float(g['x'].mean()) if n*f else None,
                'weights_density':float(g['w'].mean()) if f*q else None})
        return {'eligible_queries':len(self.eligible),'groups':len(groups),
            'distinct_projected_indices':len(self.indices),**dict(self.stats),
            **{k:sum(g[k] for g in groups) for k in ('rows','distinct_context_profiles','positive_root_rule_pairs',
                'raw_score_outputs','shared_score_outputs','raw_gemm_flops','shared_gemm_flops','cofactor_gemm_flops')},
            'by_group':sorted(groups,key=lambda x:x['raw_score_outputs'],reverse=True)}


def reference(queries,tables,eligible):
    """Independent full-query natural join, projecting only anchor row IDs.

    Uses indexed relations, but no ResponseBank feature or profile results.
    Repeated variables, literals and whole binding dictionaries are retained.
    """
    found=set();cache={}
    def get(atom,binding):
        key=(atom['table'],tuple((c.get('Slot'),c.get('Literal')) for c in atom['columns']),
             tuple(sorted(variables(atom)&binding.keys())))
        if key not in cache:
            slots=key[-1];idx=defaultdict(list)
            for rid,row in enumerate(tables.get(atom['table'],[])):
                b=row_binding(atom,row)
                if b is not None:idx[tuple(b[s] for s in slots)].append(b)
            cache[key]=idx
        return cache[key].get(tuple(binding[s] for s in key[-1]),())
    def exists(atoms,binding):
        if not atoms:return True
        for new in get(atoms[0],binding):
            if all(s not in binding or binding[s]==v for s,v in new.items()):
                if exists(atoms[1:],{**binding,**new}):return True
        return False
    for qi,ai in eligible.items():
        atoms=queries[qi]['shape']['atoms'];a=atoms[ai]
        for rid,row in enumerate(tables.get(a['table'],[])):
            b=row_binding(a,row)
            if b is not None and exists([x for i,x in enumerate(atoms) if i!=ai],b):found.add((qi,rid))
    return found
