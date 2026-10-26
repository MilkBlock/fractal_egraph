"""Stream full-scale stage observations into weighted ridge sufficient statistics."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import time
import numpy as np
from features import STAGES
from study import fit_model, counts, codes, fingerprint, sha

VARIANTS=['shape','entry','polyspine','residual1','residual8','versioned8','shuffled8']
SPLITS=['time','lineage']
KEY=np.dtype([('base','<u4'),('aux','<u4'),('tail','<f2',(32,))])


def decode_packet(path):
    raw=path.read_bytes()
    if raw[:8]!=b'POLYRES1':raise ValueError('packet schema')
    n,p,g,t,nr,nc,nq,depth=struct.unpack_from('<8I',raw,8);at=40
    def take(count,dtype):
        nonlocal at
        a=np.frombuffer(raw,dtype=dtype,count=count,offset=at);at+=4*count;return a
    seed=take(n*p,'<f4');desc=take(g*9,'<u4').reshape(-1,9);rows=take(nr,'<u4');code=take(nc,'<u4');queries=take(nq,'<u4')
    if at!=len(raw):raise ValueError('trailing packet')
    return n,seed,desc,rows,code,queries


def reference(packet):
    n,seed,descs,rows,code,queries=decode_packet(packet);targets=[];dep_columns=[];lengths=[]
    for d in descs:
        start,end,base,width,pc,na,nd,pred,col=map(int,d)
        table=rows[base:base+(end-start)*width].reshape(-1,width);valid=np.ones(len(table),bool)
        for i in range(na):valid&=table[:,code[pc+2*i]]==table[:,code[pc+2*i+1]]
        table=table[valid];pc+=na*2
        targets.extend((pred*n+table[:,col]).tolist())
        deps=np.column_stack([code[pc+2*j]*n+table[:,code[pc+2*j+1]] for j in range(nd)]) if nd else np.empty((len(table),0),np.uint32)
        dep_columns.extend(deps.tolist());lengths.extend([nd]*len(table))
    targets=np.array(targets,np.uint32);lengths=np.array(lengths,np.uint32);arity=int(lengths.max(initial=0))
    dep=np.zeros((len(targets),arity),np.uint32)
    for i,row in enumerate(dep_columns):dep[i,:len(row)]=row
    mask=np.arange(arity)[None,:]<lengths[:,None];state=seed.copy();result=np.zeros((len(queries),17),np.float32);result[:,0]=state[queries]
    for depth in range(1,9):
        total=np.where(mask,state[dep],0).sum(1,dtype=np.float32)
        nxt=state.copy();fraction=np.zeros_like(state)
        np.maximum.at(nxt,targets,(total==lengths).astype(np.float32))
        ready=np.ones(len(targets),np.float32);np.divide(total,lengths,out=ready,where=lengths>0)
        np.maximum.at(fraction,targets,ready)
        result[:,2*depth-1]=nxt[queries];result[:,2*depth]=fraction[queries];state=nxt
    return result


def evaluate(dataset,projection,out,worker):
    out.mkdir(parents=True,exist_ok=False);d=json.loads((dataset/'dataset.json').read_text())
    if sha(dataset/'lineage.npz')!=d['lineage_sha256']:raise ValueError('lineage artifact changed')
    lineage=np.load(dataset/'lineage.npz');held=lineage['heldout'];groups=lineage['groups']
    token_hash=np.array([fingerprint(t) for t in d['vocabulary']],np.uint64);tc=codes(token_hash,256)
    sc=np.zeros((len(d['signatures']),256),np.float16)
    for i,bag in enumerate(d['signatures']):
        div=1
        while div*div<len(bag):div*=2
        if bag:sc[i]=tc[bag].sum(0)/div
    table={};keyrows=[];stats=defaultdict(lambda:defaultdict(lambda:[0,0]));audit=[]
    populations=defaultdict(lambda:{'rows':0,'positive':0,'groups':set(),'positive_events':set()})
    direct=defaultdict(lambda:np.zeros(4,np.int64));started=time.perf_counter();previous={}
    def aggregate(key,ids,y):
        if not len(ids):return
        u,inv=np.unique(ids,return_inverse=True);n=np.bincount(inv);p=np.bincount(inv,weights=y)
        counter=stats[key]
        for i,a,b in zip(u,n,p):counter[int(i)][0]+=int(a);counter[int(i)][1]+=int(b)
    for f in d['frames']:
        if sha(dataset/f['file'])!=f['file_sha256']:raise ValueError('frame changed')
        z=np.load(dataset/f['file']);eligible=z['eligible'];y=z['gain'];root=z['roots'];feat=z['features'];n=len(root)
        version=np.zeros((n,4),np.float16)
        for j,rid in enumerate(root):
            key=(f['stage'],int(rid));current=tuple(map(int,feat[j]))+(int(z['before_count'][j]),)
            if key in previous:
                old=previous[key];version[j]=[1,current[:2]!=old[:2],current[2]!=old[2],current[3]!=old[3]]
            # Only PRE-stage metadata is retained. Future union lineage is used
            # for data splitting, never to reconstruct this historical input.
            previous[key]=current
        gpu=np.zeros((n,17),np.float32)
        if f['projection_supported']:
            file=projection/f['stage']/f"cycle-{f['id']:02}-prestate.bin"
            gpu=np.fromfile(file,dtype='<f4').reshape(n,17)
            expected=reference(dataset/f['stage']/f"frame-{f['id']:03}.bin")
            np.testing.assert_array_equal(gpu[:,[0,1,3,5,7,9,11,13,15]],expected[:,[0,1,3,5,7,9,11,13,15]])
            np.testing.assert_allclose(gpu,expected,atol=1e-7,rtol=0)
            audit.append({'frame':f['id'],'stage':f['stage'],'rows':n,'max_error':float(np.abs(gpu-expected).max(initial=0))})
        masks={'time':{'train':eligible&(f['cycle']<6),'test':eligible&(f['cycle']>=6)},
               'lineage':{'train':eligible&~held[root],'test':eligible&held[root]}}
        for split,parts in masks.items():
            for part,take in parts.items():
                pop=populations[(split,f['stage'],part)];pop['rows']+=int(take.sum());pop['positive']+=int(y[take].sum());pop['groups'].update(map(int,groups[root[take]]))
                pos=take&y;pop['positive_events'].update(zip(map(int,groups[root[pos]]),map(int,z['event'][pos])))
            if f['projection_supported']:
                t=parts['test'];truth=y[t];pred=(gpu[t,15]>0)&(gpu[t,0]==0)
                direct[(split,f['stage'])]+=np.array([(truth&pred).sum(),(~truth&pred).sum(),(truth&~pred).sum(),(~truth&~pred).sum()])
        permutation=np.random.default_rng(2111+f['id']).permutation(n)
        for v,name in enumerate(VARIANTS):
            rec=np.zeros(n,KEY);rec['base']=feat[:,0];rec['aux']=np.uint32(0xffffffff)
            if name!='shape':
                rec['aux']=feat[:,1 if name=='entry' else 2]
                rec['tail'][:,0]=(z['before_count']>0);rec['tail'][:,1]=np.log2(1+z['before_count'])/16
            if name in ['residual1','residual8','versioned8','shuffled8']:
                source=gpu[permutation] if name=='shuffled8' else gpu;depth=1 if name=='residual1' else 8
                for k in range(1,depth+1):
                    rec['tail'][:,2*k]=np.maximum(0,source[:,2*k-1]-source[:,0]);rec['tail'][:,2*k+1]=source[:,2*k]
            if name=='versioned8':rec['tail'][:,18:22]=version
            unique,inv=np.unique(rec.view(np.dtype((np.void,KEY.itemsize))).ravel(),return_inverse=True)
            ids=[]
            for k in unique:
                b=bytes(k)
                if b not in table:table[b]=len(keyrows);keyrows.append(b)
                ids.append(table[b])
            ids=np.array(ids,np.uint32)[inv]
            for split,parts in masks.items():
                for part,take in parts.items():aggregate((name,f['stage'],split,part),ids[take],y[take])
        if f['id']%20==0:print('prepared',f['id'],'contexts',len(keyrows),flush=True)
    preparation=time.perf_counter()-started
    for stage in STAGES:
        a=populations[('lineage',stage,'train')]['groups'];b=populations[('lineage',stage,'test')]['groups']
        if a&b:raise AssertionError('union lineage crosses split')
    keys=np.frombuffer(b''.join(keyrows),dtype=KEY);x=np.zeros((len(keys),512),np.float16)
    x[:,:256]=sc[keys['base']];nonempty=keys['aux']!=np.uint32(0xffffffff)
    x[nonempty,256:]=sc[keys['aux'][nonempty]];x[:,-32:]=keys['tail']
    models={};measurements=[];jobs=[]
    def metric(counter,scores):
        ids=np.fromiter(counter,dtype=np.int64);n=np.array([counter[int(i)][0] for i in ids]);p=np.array([counter[int(i)][1] for i in ids])
        return counts(np.r_[np.ones(len(ids)),np.zeros(len(ids))],np.r_[scores[ids],scores[ids]],np.r_[p,n-p])
    for split in SPLITS:
        for stage in STAGES:
            for name in VARIANTS:
                train=stats[(name,stage,split,'train')];test=stats[(name,stage,split,'test')]
                if not train or not test:continue
                digest=hashlib.sha256(repr(sorted(train.items())).encode()).hexdigest();cache=(stage,split,digest)
                if cache not in models:
                    start=time.perf_counter();n=np.zeros(len(keys),np.int64);p=n.copy()
                    for i,(count,positive) in train.items():n[i]=count;p[i]=positive
                    w,b,prior=fit_model(x,n,p);scores=x.astype(np.float32)@w.astype(np.float32)/512+b
                    models[cache]={'w':w,'b':b,'scores':scores,'seconds':time.perf_counter()-start,'test':set()}
                m=models[cache];m['test'].update(test)
                measurement={'split':split,'stage':stage,'variant':name,'test':metric(test,m['scores']),
                             'model':list(models).index(cache),'train_rows':sum(v[0] for v in train.values())}
                measurements.append(measurement)
                print(split,stage,name,measurement['test'],flush=True)
    references=[]
    with (out/'input.bin').open('wb') as stream:
        stream.write(b'SPINE004');stream.write(struct.pack('<I',len(models)))
        for m in models.values():
            used=sorted(m['test']);stream.write(struct.pack('<3I',len(used),512,1))
            for i in used:
                for sig in [keys['base'][i],keys['aux'][i]]:
                    bag=[] if sig==np.uint32(0xffffffff) else d['signatures'][int(sig)]
                    stream.write(struct.pack('<I',len(bag)));stream.write(token_hash[bag].astype('<u8').tobytes())
                stream.write(keys['tail'][i].astype('<f2').tobytes())
            stream.write(m['w'].astype('<f2').tobytes());stream.write(np.array([m['b']],'<f4').tobytes());references.append(m['scores'][used])
            jobs.append({'rows':len(used),'width':512,'heads':1})
    np.concatenate(references).astype('<f4').tofile(out/'reference.bin')
    with (out/'gpu.log').open('w') as log:
        subprocess.run([str(worker.resolve()),str(out/'input.bin'),str(out/'reference.bin'),str(out/'gpu.json')],check=True,stdout=log,stderr=log)
    gpu=json.loads((out/'gpu.json').read_text())
    if gpu['threshold_disagreements']:raise AssertionError('GPU score decision mismatch')
    result={'schema':'polyspine-scale-results/v1','dataset':str(dataset.resolve()),'dataset_sha256':sha(dataset/'dataset.json'),
            'source_sha256':d['source_sha256'],'frontend_sha256':d['frontend_sha256'],'native_runs':d['native_runs'],
            'native_trace_exact':d['native_trace_exact'],'frames':len(d['frames']),'lineages':d['lineages'],
            'populations':[{'split':s,'stage':t,'part':p,**{k:len(v) if isinstance(v,set) else v for k,v in stats.items()}} for (s,t,p),stats in populations.items()],
            'measurements':measurements,'direct_projection':[{'split':s,'stage':t,'tp':int(v[0]),'fp':int(v[1]),'fn':int(v[2]),'tn':int(v[3])} for (s,t),v in direct.items()],
            'projection_audit':audit,'distinct_context_vectors':len(keys),'distinct_models':len(models),
            'observation_seconds':sum(f['observation_seconds'] for f in d['frames']),'feature_seconds':sum(f['feature_seconds'] for f in d['frames']),
            'stream_aggregation_and_audit_seconds':preparation,'ridge_fit_seconds':sum(m['seconds'] for m in models.values()),
            'wire_bytes':d['wire_bytes'],'gpu':gpu,'jobs':jobs,'score_worker_sha256':sha(worker),
            'scope':d['scope'],'target_semantics':d['target_semantics'],
            'limits':['Lineage holdout uses other roots across this same program, not unseen-program validation.',
                      'Time holdout may contain previously seen roots; reported separately.',
                      'Projection only supports unary purity/resolved families; type/extraction use learned observed interfaces.',
                      'New roots outside pre-stage domain are counted separately, not called covered.',
                      'No matches/effects are skipped or committed; projected output gains are not full binding matches.',
                      'Diagnostics and offline fitting do not establish an end-to-end speedup.']}
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--dataset',type=Path,required=True);p.add_argument('--projection',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--worker',type=Path,required=True)
    a=p.parse_args();evaluate(a.dataset,a.projection,a.output,a.worker)
