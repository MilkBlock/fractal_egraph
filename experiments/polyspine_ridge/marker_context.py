"""Export resumable typed marker wiring, including unresolved input ports.

All alternatives/bindings remain in the export. The optional min-cost projection
is deliberately narrower than full candidate/union semantics.
"""
from collections import defaultdict
import numpy as np
from affine_terms import compile_program,pair_index,exact,HI
from features import identity


def compile_links(core):
    try:p=compile_program(core)
    except (KeyError,StopIteration) as error:raise ValueError('unsupported terms interface') from error
    if p['max_registers']>16 or not all(r['matrix_range_certificate'] for r in p['recipes']):raise ValueError('GPU arithmetic representation not certified')
    p['heads']=[r['head'] for rs in core['rulesets'] if rs['name']=='terms' for r in rs['rules']]
    defaults=[]
    for rs in core['rulesets']:
        if rs['name']=='terms-helpers':
            for r in rs['rules']:
                for a in r['head']:
                    if a.get('call',{}).get('table')=='TCPair' and 'literal' in a['args'][1]:defaults.append(int(a['args'][1]['literal']))
    if len(set(defaults))!=1:raise ValueError('requires one observed native initializer cost')
    if not 0<=defaults[0]<=HI:raise ValueError('initializer outside nonnegative i64 domain')
    p['initializer_cost']=defaults[0]
    # This projection uses monotonicity of these actual cost expressions. Do
    # not silently apply min-of-inputs to a nonmonotone or guarded recipe.
    for r in p['recipes']:
        if any(c<0 for c in r['coefficients']) or any(op!=0 or (ak and a<0) or (bk and b<0) for op,ak,a,bk,b in r['operations']):raise ValueError('nonmonotone cost program')
        names=[x for port in r['inputs'] for x in [port['term'],port['cost'],port['return']]]
        if len(names)!=len(set(names)):raise ValueError('cross-port alias constraints require a richer projection')
    p['copy_recipe']=len(p['recipes']);p['min_recipe']=len(p['recipes'])+1
    p['schema']='resumable-terms-context/v1'
    return p


def csr(keys,n):
    return np.r_[0,np.cumsum(np.bincount(keys,minlength=n))].astype(np.uint32),np.argsort(keys,kind='stable').astype(np.uint32)


def export_context(snapshot,keys,program):
    visible=[r for r in snapshot['rows'] if not r['subsumed']];slots=list(keys);slot={k:i for i,k in enumerate(slots)}
    def coord(k):
        if k not in slot:slot[k]=len(slots);slots.append(k)
        return slot[k]
    rows=[];bindings=[];seeds=[];functions=[];terms=[];termids={};lookup,_=pair_index(snapshot)
    def termid(t):
        if t not in termids:termids[t]=len(terms);terms.append(t)
        return termids[t]
    for r in visible:
        v=list(map(identity,r['values']))
        if r['table']=='TCPair':
            cost=int(r['values'][1]['value'])
            if cost<0:raise ValueError('negative cost: native panic behavior cannot be projected as min')
            seeds.append([coord(v[2]),termid(v[0]),cost])
        if r['table']=='ExtractedExpr':functions.append((coord(v[0]),coord(v[1])))
    def append(recipe,out,children,original):
        rows.append([coord(out),recipe,*[coord(k) for k in children],*[0]*(3-len(children)),len(children)])
        bindings.append(original)
    recipes={r['anchor']:(i,r) for i,r in enumerate(program['recipes'])}
    for row in visible:
        v=list(map(identity,row['values']));table=row['table']
        if table in recipes:
            i,r=recipes[table];env={};ok=True
            for a,k in zip(r['args'],v):
                if a['var'] in env and env[a['var']]!=k:ok=False
                env[a['var']]=k
            if ok:append(i,v[-1],[v[x['column']] for x in r['inputs']],{'table':table,'values':v})
        elif table=='Smaller':append(program['min_recipe'],v[-1],v[:2],{'table':table,'values':v})
        elif table=='ExtractedExpr':append(program['copy_recipe'],v[0],[v[1]],{'table':table,'values':v})
    rows=np.array(rows,np.uint32).reshape(-1,6);seed=np.array(seeds,np.int64).reshape(-1,3)
    n=len(slots);initial=np.zeros((n,4),np.int64);initial[:,0]=HI;initial[:,2]=-1
    for index,(_,t,c) in enumerate(seed):
        node=int(seed[index,0])
        if not initial[node,1] or c<initial[node,0]:initial[node]=[c,1,-index-2,0]
    # A visible ExtractedExpr -> TCPair is already available before projection.
    # Pending Smaller nodes are kept as dependencies, not solved on CPU here.
    for node,tc in functions:
        if initial[tc,1] and (not initial[node,1] or initial[tc,0]<initial[node,0]):initial[node]=initial[tc]
    offsets,index=csr(rows[:,0],n)
    edges=np.array([(int(r[j+2]),i) for i,r in enumerate(rows) for j in range(int(r[5]))],np.uint32).reshape(-1,2)
    dep_offsets,order=csr(edges[:,0],n);dependents=edges[order,1]
    # Extra static information is available for prediction; all term identities
    # are retained separately and never treated as ordinal floating features.
    static=np.zeros((n,4),np.float32)
    for key,choices in lookup.items():
        i=coord(key);static[i,0]=len(choices);static[i,1]=len({t for t,c in choices})
    for row in rows:
        out=int(row[0]);static[out,2]+=1
        static[out,3]+=sum(not initial[int(row[j+2]),1] for j in range(int(row[5])))
    return {'rows':rows,'initial':initial,'seeds':seed,'offsets':offsets,'index':index,
            'dependency_offsets':dep_offsets,'dependents':dependents,'static':static,
            'query':np.arange(len(keys),dtype=np.uint32),'slots':slots,'bindings':bindings,'terms':terms}


def reference_step(c,program,current):
    rows=c['rows'];candidate=np.zeros((len(rows),3),np.int64);nxt=current.copy()
    for i,row in enumerate(rows):
        out,rid,a,b,cc,arity=map(int,row);ports=[a,b,cc][:arity]
        if not all(current[k,1] for k in ports):continue
        costs=[int(current[k,0]) for k in ports]+[0]*(3-arity)
        if rid==program['copy_recipe']:value=costs[0]
        elif rid==program['min_recipe']:value=min(costs[:2])
        else:
            value,_,flags=exact(program['recipes'][rid],costs,0,has_old=False)
            if not flags&1:continue
            value=min(value,program['initializer_cost'])
        candidate[i]=[value,1,0]
        if not nxt[out,1] or value<nxt[out,0]:nxt[out]=[value,1,i,current[out,3]+1]
    return nxt,candidate


def reference_features(c,current,candidate,previous):
    # Only queried Expr slots are output; TermAndCost auxiliary slots stay in
    # the shared state so pending Smaller computations can feed Expr inputs.
    n=len(c['query']);f=np.zeros((n,24),np.float32);initial=c['initial'];rows=c['rows'];slog=lambda v:np.log2(1+float(v))/64
    for out in range(n):
        ids=c['index'][c['offsets'][out]:c['offsets'][out+1]];rs=rows[ids];cs=candidate[ids];valid=cs[:,1].astype(bool);vals=cs[valid,0];before=initial[out];now=current[out]
        f[out,:8]=[np.log2(1+len(ids))/16,before[1],slog(before[0]) if before[1] else 0,now[1],slog(now[0]) if now[1] else 0,
                    bool(now[1] and not before[1]),bool(now[1] and before[1] and now[0]<before[0]),min(1,now[3]/16)]
        f[out,8]=valid.mean() if len(valid) else 0;f[out,9]=np.log2(1+valid.sum())/16
        if len(vals):
            f[out,10:13]=[slog(vals.min()),slog(vals.max()),np.mean([slog(v) for v in vals])]
            if before[1]:f[out,13:16]=[np.any(vals<before[0]),np.any(vals==before[0]),np.any(vals>before[0])]
        f[out,16]=bool(now[1]!=previous[out,1] or now[0]!=previous[out,0]);f[out,17:19]=np.log2(1+c['static'][out,:2])/16
        arity=sum(int(r[5]) for r in rs);ready=0;new=0;improved=0;best=0
        for r in rs:
            ports=list(map(int,r[2:2+int(r[5])]))
            ready+=sum(current[p,1] for p in ports);new+=sum(current[p,1] and not initial[p,1] for p in ports)
            improved+=sum(current[p,1] and initial[p,1] and current[p,0]<initial[p,0] for p in ports)
            best=max(best,sum(current[p,1] for p in ports)/len(ports) if ports else 1)
        f[out,19:24]=[ready/max(1,arity),new/max(1,arity),improved/max(1,arity),best,np.log2(1+c['static'][out,3])/16]
    return f



def write_packet(c,program,path):
    import struct
    width=(len(program['recipes'])*4+7)//8*8;weight=np.zeros((width,8),'<f2')
    for i,r in enumerate(program['recipes']):weight[i*4:i*4+4,0]=r['coefficients']
    with open(path,'wb') as s:
        s.write(b'MRKLINK1');s.write(struct.pack('<5Iq',len(c['rows']),len(c['initial']),len(c['query']),width,len(program['recipes']),program['initializer_cost']))
        for a in [c['rows'].astype('<u4'),c['initial'].astype('<i8'),c['offsets'].astype('<u4'),c['index'].astype('<u4'),c['static'].astype('<f4'),weight]:s.write(a.tobytes())


def save_export(c,program,out,frame_id):
    """Shared by live Polyspine analysis and cached-evidence replay."""
    import gzip,json,hashlib
    from pathlib import Path
    out=Path(out);out.mkdir(parents=True,exist_ok=True)
    name=f'frame-{frame_id}.npz';packet=f'frame-{frame_id}.bin'
    np.savez_compressed(out/name,**{k:v for k,v in c.items() if isinstance(v,np.ndarray)})
    with gzip.open(out/f'frame-{frame_id}-wiring.json.gz','wt') as s:
        json.dump({k:v for k,v in c.items() if not isinstance(v,np.ndarray)},s,separators=(',',':'))
    write_packet(c,program,out/packet)
    digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    return {'id':frame_id,'packet':packet,'file':name,'sha256':digest(out/name),'packet_sha256':digest(out/packet),
            'rows':len(c['query']),'slots':len(c['initial']),'markers':len(c['rows']),
            'dependencies':len(c['dependents']),'seed_alternatives':len(c['seeds'])}


def save_manifest(program,frames,out,**metadata):
    import json
    from pathlib import Path
    from affine_terms import metal_program
    out=Path(out);out.mkdir(parents=True,exist_ok=True)
    (out/'compiled.metal').write_text(Path(__file__).with_name('marker.metal').read_text().replace('// COMPILED_PROGRAM',metal_program(program)))
    result={'schema':'resumable-marker-study/v1','program':program,'frames':frames,
            'feature_boundary':'PRE-stage evidence only; no post-stage state is in device input',**metadata}
    (out/'input.json').write_text(json.dumps(result,indent=2));return result
