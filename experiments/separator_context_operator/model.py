"""Exact projected binding relations, not parameterized Cell families.

Categorical axes are arbitrary typed identities. An orthogonal character basis
makes the *same* relation join into signed dense GEMM. It is a change of basis,
not a claim that the rank or the amount of necessary computation decreased.
"""
from __future__ import annotations
import numpy as np


def hadamard(size):
    width=1
    while width<size:width*=2
    h=np.ones((1,1),dtype=np.int64)
    while h.shape[0]<width:h=np.block([[h,h],[h,-h]])
    return h


def signed_join(left,right):
    """Join on one WHOLE separator key; preserve both outer row coordinates."""
    if left.shape[1]!=right.shape[1]:raise ValueError('different separator axes')
    h=hadamard(left.shape[1]);k=len(h)
    lp=np.pad(left,((0,0),(0,k-left.shape[1])))
    rp=np.pad(right,((0,0),(0,k-right.shape[1])))
    x,y=lp@h,rp@h
    # Integer reference. GPU uses FP32 only under this sufficient exact bound.
    bound=k*int(np.max(np.abs(x),initial=0))*int(np.max(np.abs(y),initial=0))
    raw=x@y.T
    if np.any(raw%k):raise AssertionError('orthogonality was lost')
    return raw//k,x,y,{'width':k,'fp32_safe_bound':bound,'fp32_safe':bound<2**24}


def signed_remap(encoded, mapping):
    """Canonical key merge in the SAME allocated basis: E' = E H^T M H / K.

    Counts stay as bags (not silently deduplicated); >0 has set semantics.
    Removal must use authoritative source row deltas, not guessed inverse union.
    """
    k=encoded.shape[1];h=hadamard(k)
    if h.shape!=(k,k):raise ValueError('power-of-two basis required')
    m=np.zeros((k,k),dtype=np.int64)
    for old in range(k):m[old,mapping.get(old,old)]=1
    numerator=encoded@(h.T@m@h)
    if np.any(numerator%k):raise AssertionError('nonintegral coordinate update')
    return numerator//k


def scatter(rows,columns,pairs):
    result=np.zeros((rows,columns),dtype=np.int64)
    if len(pairs):
        p=np.asarray(pairs,dtype=np.int64)
        np.add.at(result,(p[:,0],p[:,1]),1)
    return result


def loop_factors(data, axes=None):
    """Compile raw columns into matrices. No multi-relation binding enumeration.

    Inputs are already canonical IDs, from native rebuild or the fixture.
    ContextOf and HasArgType are allowed to be genuinely multi-valued.
    """
    expr=sorted({v for row in data['loops'] for v in row[1:3]}
                |{r[0] for r in data['contexts']}|{r[0] for r in data['types']})
    ctx=sorted({r[1] for r in data['contexts']}|{r[3] for r in data['constants']})
    ty=sorted({r[1] for r in data['types']})
    if axes is not None:expr,ctx,ty=axes
    ei={v:i for i,v in enumerate(expr)};ci={v:i for i,v in enumerate(ctx)};ti={v:i for i,v in enumerate(ty)}
    c=scatter(len(expr),len(ctx),[(ei[e],ci[k]) for e,k in data['contexts']])
    types=scatter(len(expr),len(ty),[(ei[e],ti[t]) for e,t in data['types']])
    constants=scatter(len(data['constants']),len(ctx),[(i,ci[row[3]]) for i,row in enumerate(data['constants'])])
    left=c[[ei[row[1]] for row in data['loops']]]
    output=c[[ei[row[2]] for row in data['loops']]]
    input_types=types[[ei[row[1]] for row in data['loops']]]
    compatibility,x,y,numeric=signed_join(output,constants)
    ready=(left>0).any(axis=1)&(input_types>0).any(axis=1)
    valid=(compatibility>0)&ready[:,None]
    values=sorted({row[1] for row in data['constants']});value_i={v:i for i,v in enumerate(values)}
    const_value=scatter(len(data['constants']),len(values),[(i,value_i[row[1]]) for i,row in enumerate(data['constants'])])
    # Cut RHS before Const creation. Quantify original Const rows and root,
    # keeping exactly (input context,input type,value). Two contractions reuse
    # construction work across different roots and old Const output types.
    groups=((left>0)[:,:,None]&(input_types>0)[:,None,:]).reshape(len(data['loops']),-1).astype(np.int64)
    root_values=valid.astype(np.int64)@const_value
    new_const=groups.T@root_values
    # These three factors together represent all RHS coordinates. Do NOT join
    # them into a cached full binding table between iterations.
    return {'left':left,'types':input_types,'compatible':valid,'q':compatibility,'output':output,
            'constant_context':constants,'axes':(expr,ctx,ty),
            'ctx_ids':ctx,'type_ids':ty,'x':x,'y':y,'numeric':numeric,
            'new_const':new_const>0,'value_ids':values,'groups':groups,
            'root_values':root_values,'const_value':const_value,
            'raw_table_cells':int(c.size+types.size+constants.size),
            'latent_cells':int(left.size+input_types.size+valid.size)}


def new_const_keys(factors):
    result=set();nt=len(factors['type_ids'])
    for g,c in np.argwhere(factors['new_const']):
        result.add((factors['value_ids'][c],factors['type_ids'][g%nt],factors['ctx_ids'][g//nt]))
    return result


def delta_terms(data,fresh):
    """Disjoint semi-naive ownership without expanding the 4-D binding tensor.

    Old means CURRENT LIVE rows below the watermark, not a stale old graph.
    Canonicalization/removal has to be applied before selecting these subsets.
    """
    full=loop_factors(data)
    old={k:[row for i,row in enumerate(data[k]) if i not in fresh.get(k,set())]
         for k in ('contexts','types','constants')}
    # Preserve constant payload coordinates and root rows even when fresh.
    old['loops']=data['loops'];old['constants']=data['constants']
    prev=loop_factors(old,full['axes'])
    cc=full['constant_context'].copy()
    for i in fresh.get('constants',set()):cc[i]=0
    q_old=prev['output']@cc.T
    _,old_x,old_y,_=signed_join(prev['output'],cc)
    full['old_x'],full['old_y']=old_x,old_y
    fresh_root=np.array([i in fresh.get('loops',set()) for i in range(len(data['loops']))],dtype=np.int64)[:,None]
    old_root=1-fresh_root
    l,h,q=full['left'],full['types'],full['q']
    lo,ho=prev['left'],prev['types']
    # L H Q - Lold Hold Qold, split into nonnegative products. Root is its own
    # disjoint ownership case. q-q_old already combines ContextOf/Const deltas.
    terms=[(fresh_root*l,h,q),
           (old_root*(l-lo),h,q),
           (old_root*lo,h-ho,q),
           (old_root*lo,ho,q-q_old)]
    assert all(np.all(x>=0) for term in terms for x in term)
    return full,terms


def delta_effects(data,full,terms):
    answer=set()
    for l,h,q in terms:
        for r,c in np.argwhere(q>0):
            for i in np.flatnonzero(l[r]):
                for t in np.flatnonzero(h[r]):
                    const=data['constants'][c]
                    answer.add((data['loops'][r][0],full['ctx_ids'][i],full['type_ids'][t],const[1],const[0]))
    return answer


def delta_reference(data,fresh):
    contexts={};types={};constants={}
    for i,(e,c) in enumerate(data['contexts']):contexts.setdefault(e,[]).append((i,c))
    for i,(e,t) in enumerate(data['types']):types.setdefault(e,[]).append((i,t))
    for i,row in enumerate(data['constants']):constants.setdefault(row[3],[]).append((i,row))
    answer=set()
    for ri,(root,inp,body) in enumerate(data['loops']):
        for ii,ic in contexts.get(inp,[]):
            for oi,oc in contexts.get(body,[]):
                for ti,ty in types.get(inp,[]):
                    for ci,const in constants.get(oc,[]):
                        if (ri in fresh.get('loops',set()) or ii in fresh.get('contexts',set())
                            or oi in fresh.get('contexts',set()) or ti in fresh.get('types',set())
                            or ci in fresh.get('constants',set())):
                            answer.add((root,ic,ty,const[1],const[0]))
    return answer


def loop_effects(data, factors):
    """Output-only expansion, never used to build the matrix inputs."""
    result=set()
    for r,k in np.argwhere(factors['compatible']):
        const=data['constants'][k]
        for c in np.flatnonzero(factors['left'][r]):
            for t in np.flatnonzero(factors['types'][r]):
                result.add((data['loops'][r][0],factors['ctx_ids'][c],factors['type_ids'][t],const[1],const[0]))
    return result


def loop_reference(data):
    # Deliberately separate indexed relational oracle; no character coding.
    contexts={};types={};constants={}
    for e,c in data['contexts']:contexts.setdefault(e,set()).add(c)
    for e,t in data['types']:types.setdefault(e,set()).add(t)
    for row in data['constants']:constants.setdefault(row[3],[]).append(row)
    result=set()
    for root,inp,body in data['loops']:
        for ictx in contexts.get(inp,[]):
            for octx in contexts.get(body,[]):
                for ty in types.get(inp,[]):
                    for const in constants.get(octx,[]):result.add((root,ictx,ty,const[1],const[0]))
    return result


def if_projection(data):
    """Existentially remove old Arg contexts, retaining the JOINT (j,type) key.

    Primitive index values need not form an interval or an affine progression.
    Multiple Arg types in one eclass are retained. Get is a canonical
    constructor: each (parent,index) key has exactly one output eclass.
    """
    positions=sorted({i for p,i,v in data['gets']})
    types=sorted({t for v,t,c in data['args']})
    values=sorted({v for p,i,v in data['gets']})
    pi={v:i for i,v in enumerate(positions)};ti={v:i for i,v in enumerate(types)};vi={v:i for i,v in enumerate(values)}
    arg_types={}
    for arg,t,c in data['args']:arg_types.setdefault(arg,set()).add(t)
    get={}
    for p,i,v in data['gets']:
        if (p,i) in get and get[p,i]!=v:raise ValueError('Get inputs must be canonicalized after rebuild')
        get[p,i]=v
    # This is a direct constructor-key read of typed Arg columns and scatter
    # into a joint signature, not a search over pairs of Get records.
    signatures=np.zeros((len(values),len(positions),len(types)),dtype=np.uint8)
    for p,j,v in data['gets']:
        for t in arg_types.get(p,[]):signatures[vi[v],pi[j],ti[t]]=1
    state=set(data['state_values'])
    answer=set();pairs=[];signature_rows=[]
    for root,pred,inputs,then,els in data['ifs']:
        for i in positions:
            a,b=get.get((then,i)),get.get((els,i))
            if a is None or b is None or a not in state:continue
            # One shared signature calculation feeds BOTH original consumers.
            l=signatures[vi[a]];r=signatures[vi[b]]
            mask=np.any(l&r,axis=1)
            for j in np.flatnonzero(mask):answer.add((root,inputs,i,positions[j]))
            pairs.append((a,b));signature_rows.append((l,r))
    return answer,{'signature_shape':list(signatures.shape),'signature_entries':int(signatures.sum()),
                   'signature_cells':int(signatures.size),'compared_value_pairs':len(pairs),
                   'distinct_value_pairs':len(set(pairs)),
                   'aligned_inner_products':len(pairs)*len(positions),
                   'type_axis':len(types)},signature_rows


def if_reference(data):
    # Exact nested witnesses, including correlated arg type across branches.
    get={(p,i):v for p,i,v in data['gets']};witness={}
    for arg,ty,ctx in data['args']:
        for (p,j),v in get.items():
            if p==arg:witness.setdefault(v,set()).add((j,ty))
    result=set();state=set(data['state_values'])
    for root,pred,inputs,th,el in data['ifs']:
        for (p,i),a in get.items():
            if p!=th or a not in state or (el,i) not in get:continue
            b=get[el,i]
            for j,t in witness.get(a,set())&witness.get(b,set()):
                result.add((root,inputs,i,j))
    return result


def if_masked_projection(data):
    """Create fine coordinates only after the cheap common projection permits it.

    AnyArgGet[value] is shared by all If frames. The finer coordinate is the
    ENTIRE (index,type) tuple; no independence between its fields is assumed.
    """
    arg_types={}
    for a,t,c in data['args']:arg_types.setdefault(a,set()).add(t)
    get={}
    for p,i,v in data['gets']:
        if (p,i) in get and get[p,i]!=v:raise ValueError('noncanonical Get key')
        get[p,i]=v
    any_arg_get={v for p,i,v in data['gets'] if p in arg_types}
    positions=sorted({i for p,i,v in data['gets']});state=set(data['state_values'])
    frames=[];before=0
    for root,pred,inputs,th,el in data['ifs']:
        for i in positions:
            a,b=get.get((th,i)),get.get((el,i))
            if a is None or b is None or a not in state:continue
            before+=1
            if a in any_arg_get and b in any_arg_get:frames.append((root,inputs,i,a,b))
    needed={x for r,inp,i,a,b in frames for x in (a,b)}
    entries={(v,j,t) for p,j,v in data['gets'] if v in needed for t in arg_types.get(p,())}
    keys=sorted({(j,t) for v,j,t in entries});key_i={x:i for i,x in enumerate(keys)}
    vals=sorted(needed);value_i={v:i for i,v in enumerate(vals)}
    signatures=np.zeros((len(vals),len(keys)),dtype=np.int64)
    for v,j,t in entries:signatures[value_i[v],key_i[j,t]]=1
    # Fine conjunction retains each key until the RHS index projection.
    joint=np.array([signatures[value_i[a]]*signatures[value_i[b]] for r,inp,i,a,b in frames],dtype=np.int64)
    answer=set()
    for f,k in np.argwhere(joint):
        root,inp,i,_,_=frames[f];answer.add((root,inp,i,keys[k][0]))
    return answer,{'coarse_frame_count':before,'fine_frame_count':len(frames),
                   'coarse_presence_values':len(any_arg_get),'fine_signature_shape':list(signatures.shape),
                   'fine_signature_cells':int(signatures.size),'fine_nonzeros':int(signatures.sum()),
                   'fine_key_is_joint':True,'full_rectangular_signature_never_allocated':True}


def comparator_projection(data):
    """Exact existential guard elimination after quotienting the UNION target.

    This does not assume singleton TCPair eclasses, stable Cell instances or
    a recursive family. Equality-only rule retains a cost-set intersection.
    """
    by_class={}
    for klass,term,cost in data['pairs']:by_class.setdefault(klass,set()).add(cost)
    bounds={k:(min(v),max(v)) for k,v in by_class.items()}
    separate={'lt':set(),'gt':set(),'eq':set()};combined=set()
    for lhs,a,b in data['smaller']:
        if a not in bounds or b not in bounds:continue
        alo,ahi=bounds[a];blo,bhi=bounds[b]
        if alo<bhi:separate['lt'].add((lhs,a))
        if ahi>blo:separate['gt'].add((lhs,b))
        if by_class[a]&by_class[b]:separate['eq'].add((lhs,a))
        # Across these three SAME-ruleset union-only consumers, <= is enough
        # for the left target. Native logical match counts are not preserved.
        if alo<=bhi:combined.add((lhs,a))
        if ahi>blo:combined.add((lhs,b))
    return combined,separate,{'classes':len(bounds),'multi_cost_classes':sum(len(v)>1 for v in by_class.values())}


def comparator_reference(data):
    values={}
    for k,t,c in data['pairs']:values.setdefault(k,[]).append((t,c))
    result={'lt':set(),'gt':set(),'eq':set()}
    for lhs,a,b in data['smaller']:
        for ta,ca in values.get(a,[]):
            for tb,cb in values.get(b,[]):
                if ca<cb:result['lt'].add((lhs,a))
                elif ca>cb:result['gt'].add((lhs,b))
                else:result['eq'].add((lhs,a))
    return result


def fused_comparator_matrix(data):
    """Fuse all three comparison rules at their common UNION-target output.

    Ordinal coordinates preserve arbitrary i64 ordering without converting the
    original costs to FP32. They must be refreshed when the cost domain changes.
    """
    costs=sorted({c for k,t,c in data['pairs']});rank={c:i for i,c in enumerate(costs)}
    by_class={}
    for k,t,c in data['pairs']:by_class.setdefault(k,[]).append(rank[c])
    bounds={k:(min(v),max(v)) for k,v in by_class.items()}
    x=np.zeros((len(data['smaller']),5),dtype=np.int64)
    for i,(lhs,a,b) in enumerate(data['smaller']):
        if a in bounds and b in bounds:x[i]=[*bounds[a],*bounds[b],1]
    # [minLeft,maxLeft,minRight,maxRight,valid] ->
    # [maxRight-minLeft+valid, maxLeft-minRight]. Integer >0 handles both gates.
    w=np.array([[-1,0],[0,1],[0,-1],[1,0],[1,0]],dtype=np.int64)
    mask=x@w>0
    out={(lhs,target) for i,(lhs,a,b) in enumerate(data['smaller'])
         for side,target in enumerate((a,b)) if mask[i,side]}
    return out,{'feature_shape':list(x.shape),'operator_shape':list(w.shape),
                'cost_order_coordinates':len(costs),'fp32_exact_if_rank_bound':len(costs)+1<2**24,
                'union_target_rules_fused':3,'preserves_individual_logical_match_counts':False}


class LoopClosure:
    """Whole actual loop rule, including Const feeding its NEXT invocation.

    Background DoWhile/ContextOf/HasArgType stays fixed within this invocation.
    Its arbitrary identities define coordinates; no numeric recurrence is used.
    Const copies c/type/context from finite input domains, proving this carrier
    closed under this one rule. Other eggcc rules do not inherit this contract.
    """
    def __init__(self,data):
        self.data=data
        self.ctx=sorted({c for e,c in data['contexts']}|{r[3] for r in data['constants']})
        self.ty=sorted({t for e,t in data['types']}|{r[2] for r in data['constants']})
        self.values=sorted({r[1] for r in data['constants']})
        expr=sorted({v for r in data['loops'] for v in r[1:]}|{e for e,c in data['contexts']}|{e for e,t in data['types']})
        f=loop_factors(data,(expr,self.ctx,self.ty))
        self.groups=f['groups'];self.output=f['output']
        self.operator=self.groups.T@self.output
        self.state=np.zeros((len(self.ctx)*len(self.ty),len(self.values)),dtype=np.int64)
        self.alias={};groups={}
        for klass,c,t,ctx in data['constants']:groups.setdefault(klass,set()).add((c,t,ctx))
        for keys in groups.values():
            for key in keys:self.alias[key]=keys
        for klass,c,t,ctx in data['constants']:
            self.state[self.ctx.index(ctx)*len(self.ty)+self.ty.index(t),self.values.index(c)]=1
        self.previous=self.state.copy();self.round=0

    def step(self):
        self.previous=self.state.copy()
        by_context=self.state.reshape(len(self.ctx),len(self.ty),len(self.values)).max(axis=1)
        self.state=np.maximum(self.state,(self.operator@by_context)>0)
        self.round+=1

    def keys(self,state=None):
        if state is None:state=self.state
        return {(self.values[v],self.ty[g%len(self.ty)],self.ctx[g//len(self.ty)]) for g,v in np.argwhere(state)}

    def deferred_effects(self):
        # Output-only materialization for the oracle. Previous is enough while
        # background facts are fixed and Const grows monotonically. This is NOT
        # claimed for arbitrary changing backgrounds; hand back/commit then.
        source_keys=self.keys(self.previous);edges=set()
        for r,(root,inp,body) in enumerate(self.data['loops']):
            for c,old_ty,old_ctx in source_keys:
                if not self.output[r,self.ctx.index(old_ctx)]:continue
                for g in np.flatnonzero(self.groups[r]):
                    new=(c,self.ty[g%len(self.ty)],self.ctx[g//len(self.ty)])
                    old=(c,old_ty,old_ctx)
                    for n in self.alias.get(new,{new}):
                        for o in self.alias.get(old,{old}):edges.add((root,*n,*o))
        return edges
