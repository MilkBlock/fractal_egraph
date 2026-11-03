"""Matched ablation: old Polyspine, actual costs, fixed affine columns, guards.

All labels/splits remain the previously frozen scale experiment. The fixed
columns are rule compiled. Ridge is trained separately and cannot change them.
"""
import argparse,json,struct,subprocess,time
from pathlib import Path
import numpy as np
from affine_terms import exact,metal_program
from study import codes,fingerprint,sha,fit_model,counts
from scale_fit import KEY

NUMERIC=np.r_[np.arange(2,18),np.arange(22,30)]


def old_model(path,job):
    raw=path.read_bytes();assert raw[:8]==b'SPINE004';at=12
    for i in range(struct.unpack_from('<I',raw,8)[0]):
        n,k,c=struct.unpack_from('<3I',raw,at);at+=12
        for _ in range(n):
            for _ in range(2):m=struct.unpack_from('<I',raw,at)[0];at+=4+8*m
            at+=64
        weights=np.frombuffer(raw,'<f2',k*c,at).copy().reshape(k,c);at+=2*k*c
        bias=np.frombuffer(raw,'<f4',c,at).copy();at+=4*c
        if i==job:return weights[:,0],bias[0]
    raise ValueError(job)


def base_records(d,dataset):
    previous={}
    for f in d['frames']:
        if f['stage']!='terms':continue
        z=np.load(dataset/f['file']);n=len(z['roots']);rec=np.zeros(n,KEY);feat=z['features']
        rec['base']=feat[:,0];rec['aux']=feat[:,2];rec['tail'][:,0]=z['before_count']>0;rec['tail'][:,1]=np.log2(1+z['before_count'])/16
        for j,rid in enumerate(z['roots']):
            current=tuple(map(int,feat[j]))+(int(z['before_count'][j]),);key=int(rid)
            if key in previous:
                old=previous[key];rec['tail'][j,18:22]=[1,current[:2]!=old[:2],current[2]!=old[2],current[3]!=old[3]]
            previous[key]=current
        yield f,z,rec


def vector_table(d):
    hashes=np.array([fingerprint(t) for t in d['vocabulary']],np.uint64);tc=codes(hashes,256);sc=np.zeros((len(d['signatures']),256),np.float16)
    for i,bag in enumerate(d['signatures']):
        div=1
        while div*div<len(bag):div*=2
        if bag:sc[i]=tc[bag].sum(0)/div
    return sc,hashes


def encode(rec,sc):
    x=np.empty((len(rec),512),np.float16);x[:,:256]=sc[rec['base']];x[:,256:]=sc[rec['aux']];x[:,-32:]=rec['tail'];return x


def reference_features(params,meta,values,n):
    root=meta[:,0];complete=meta[:,2].astype(bool);old=meta[:,3].astype(bool);flags=values[:,2]
    valid=(flags&1)!=0;delta=(flags&16)!=0
    slog=lambda a:np.copysign(np.log2(1+np.abs(a.astype(np.float32)))/64,a)
    f=np.zeros((n,24),np.float32);num=np.bincount(root,minlength=n);den=np.maximum(1,num)
    def total(v):return np.bincount(root,weights=v,minlength=n)
    nc=total(complete);no=total(old);nv=total(valid);nd=total(delta)
    def bounds(v,mask):
        lo=np.full(n,np.inf);hi=np.full(n,-np.inf);np.minimum.at(lo,root[mask],v[mask]);np.maximum.at(hi,root[mask],v[mask]);lo[~np.isfinite(lo)]=0;hi[~np.isfinite(hi)]=0;return lo,hi
    f[:,0]=np.log2(1+num)/16;f[:,1]=nc/den;f[:,2]=no/den;f[:,3],f[:,4]=bounds(slog(params[:,3]),old)
    for j in range(3):f[:,5+j]=total(slog(params[:,j])*complete)/np.maximum(1,nc)
    v=slog(values[:,0]);f[:,8],f[:,9]=bounds(v,valid);f[:,10]=total(v*valid)/np.maximum(1,nv)
    v=slog(values[:,1]);f[:,11],f[:,12]=bounds(v,delta);f[:,13]=total(v*delta)/np.maximum(1,nd)
    f[:,14]=nv/den;f[:,15]=np.log2(1+nv)/16
    for a,b,mask in [(16,17,(flags&2)!=0),(18,19,(flags&4)!=0),(20,21,valid&old&(values[:,0]==params[:,3]))]:
        t=total(mask);f[:,a]=t>0;f[:,b]=t/den
    f[:,22]=total(complete&~valid)/den;f[:,23]=total(valid&((flags&32)==0))/den
    return f


def prepare(dataset,capture,oldfit,out,worker):
    d=json.loads((dataset/'dataset.json').read_text());c=json.loads((capture/'capture.json').read_text());assert sha(dataset/'dataset.json')==c['dataset_sha256']
    assert json.loads((oldfit/'results.json').read_text())['dataset_sha256']==sha(dataset/'dataset.json')
    out.mkdir(parents=True,exist_ok=False);sc,hashes=vector_table(d);w,b=old_model(oldfit/'input.bin',43);program=c['program']
    assert program['max_registers']<=16 and all(r['matrix_range_certificate'] for r in program['recipes'])
    width=(512+len(program['recipes'])*4+1+7)//8*8;weights=np.zeros((width,8),np.float16);weights[:512,0]=w
    for i,r in enumerate(program['recipes']):weights[512+4*i:512+4*i+4,1]=r['coefficients'];weights[512+4*i:512+4*i+4,2]=-np.array(r['coefficients'])
    weights[512+len(program['recipes'])*4,2]=1
    kernel=Path(__file__).with_name('affine.metal').read_text().replace('// COMPILED_PROGRAM',metal_program(program));(out/'compiled.metal').write_text(kernel)
    frames=[]
    for f,z,rec in base_records(d,dataset):
        cf=next(x for x in c['frames'] if x['id']==f['id']);assert sha(capture/cf['file'])==cf['sha256'];a=np.load(capture/cf['file']);p=a['params'];m=a['meta'];n=len(rec)
        x=encode(rec,sc);order=np.argsort(m[:,0],kind='stable').astype('<u4');offset=np.r_[0,np.cumsum(np.bincount(m[:,0],minlength=n))].astype('<u4')
        name=f"frame-{f['id']}.bin"
        with (out/name).open('wb') as s:
            s.write(b'AFFINE01');s.write(struct.pack('<4I',len(p),n,width,len(program['recipes'])))
            for arr in [x.astype('<f2'),p.astype('<i8'),m.astype('<u4'),weights,offset,order]:s.write(arr.tobytes())
        frames.append({'id':f['id'],'packet':name})
    (out/'input.json').write_text(json.dumps({'frames':frames}))
    subprocess.run([str(worker.resolve()),str(out/'input.json'),str(out/'gpu'),str(out/'compiled.metal')],check=True)
    audit=[]
    for f,z,rec in base_records(d,dataset):
        a=np.load(capture/f"frame-{f['id']:03}.npz");p=a['params'];m=a['meta'];n=len(rec)
        expected=np.array([exact(program['recipes'][int(row[1])],cost[:3],int(cost[3]),bool(row[2]),bool(row[3])) for cost,row in zip(p,m)],np.int64)
        actual=np.fromfile(out/'gpu'/f"frame-{f['id']}-exact.bin",'<i8').reshape(-1,3);np.testing.assert_array_equal(actual,expected)
        features=np.fromfile(out/'gpu'/f"frame-{f['id']}-features.bin",'<f4').reshape(n,24);np.testing.assert_allclose(features,reference_features(p,m,expected,n),atol=3e-6,rtol=1e-5)
        matrix=np.fromfile(out/'gpu'/f"frame-{f['id']}-matrix.bin",'<f4').reshape(-1,3)
        ref=encode(rec,sc).astype(np.float32)@w.astype(np.float32)/512+b;gpu=matrix[:,0]/512+b
        np.testing.assert_allclose(gpu,ref[m[:,0]],atol=3e-6,rtol=1e-5);assert np.all((gpu>=.5)==(ref[m[:,0]]>=.5))
        valid=(expected[:,2]&1)!=0;safe=(expected[:,2]&32)!=0
        audit.append({'id':f['id'],'contexts':len(p),'valid':int(valid.sum()),'matrix_exact':int(safe.sum()),'i64_fallback':int((valid&~safe).sum()),'integer_mismatches':0,'score_threshold_mismatches':0,'max_score_error':float(np.max(abs(gpu-ref[m[:,0]])))})
        np.savez_compressed(out/f"features-{f['id']}.npz",features=features.astype(np.float16))
    (out/'audit.json').write_text(json.dumps(audit,indent=2))


def fit(dataset,capture,oldfit,prepared,out,worker):
    out.mkdir(parents=True,exist_ok=False);d=json.loads((dataset/'dataset.json').read_text());held=np.load(dataset/'lineage.npz')['heldout'];sc,hashes=vector_table(d)
    records=[];extras=[];labels=[];lineage=[];time_train=[];before=[];improved=[];frameids=[]
    for f,z,rec in base_records(d,dataset):
        a=np.load(capture/f"frame-{f['id']:03}.npz");extra=np.load(prepared/f"features-{f['id']}.npz")['features']
        records.append(rec);extras.append(extra);labels.append(z['gain']);lineage.append(~held[z['roots']]);time_train.append(np.full(len(rec),f['cycle']<6));before.append(z['before_count']>0);improved.append(a['cost_improved']);frameids.append(np.full(len(rec),f['id']))
    rec=np.concatenate(records);extra=np.concatenate(extras);y=np.concatenate(labels);old=np.concatenate(before);ci=np.concatenate(improved);fid=np.concatenate(frameids)
    masks={'lineage':np.concatenate(lineage),'time':np.concatenate(time_train)};results=[];jobs=[];refs=[];model_data=[]
    permutation=np.arange(len(y))
    for i in np.unique(fid):
        positions=np.flatnonzero(fid==i);permutation[positions]=np.random.default_rng(4201+int(i)).permutation(positions)
    for split,train in masks.items():
        for variant,count in [('baseline',0),('raw_cost',8),('affine',16),('affine_guard',24),('shuffled',24)]:
            k=rec.copy();e=extra[permutation] if variant=='shuffled' else extra
            if count:k['tail'][:,NUMERIC[:count]]=e[:,:count]
            unique,inverse=np.unique(k.view(np.dtype((np.void,KEY.itemsize))).ravel(),return_inverse=True);keys=unique.view(KEY);x=encode(keys,sc)
            if variant=='baseline':w,b=old_model(oldfit/'input.bin',43 if split=='lineage' else 21);secs=0
            else:
                n=np.bincount(inverse[train],minlength=len(keys));p=np.bincount(inverse[train],weights=y[train],minlength=len(keys));start=time.perf_counter();w,b,_=fit_model(x,n,p);secs=time.perf_counter()-start
            scores=x.astype(np.float32)@w.astype(np.float32)/512+b;s=scores[inverse];test=~train
            result={'split':split,'variant':variant,'train_rows':int(train.sum()),'test_rows':int(test.sum()),'positive':int(y[test].sum()),'test':counts(y[test],s[test]),'fit_seconds':secs,
                    'subsets':{name:counts(y[test&mask],s[test&mask]) for name,mask in [('old_answer',old),('no_old_answer',~old),('complete_context',extra[:,1]>0),('no_complete_context',extra[:,1]==0)]},
                    'cost_decrease_test':counts(ci[test],s[test]),'by_frame':[{'id':int(i),**counts(y[test&(fid==i)],s[test&(fid==i)])} for i in np.unique(fid[test])]}
            results.append(result);print(split,variant,result['test'],flush=True)
            used=np.unique(inverse[test]);model_data.append((keys[used],w,b));refs.append(scores[used]);jobs.append({'split':split,'variant':variant,'rows':len(used)})
            np.savez_compressed(out/f'{split}-{variant}.npz',score=s[test],truth=y[test],frame=fid[test],complete=extra[test,1]>0,old=old[test])
    with (out/'input.bin').open('wb') as s:
        s.write(b'SPINE004');s.write(struct.pack('<I',len(model_data)))
        for keys,w,b in model_data:
            s.write(struct.pack('<3I',len(keys),512,1))
            for row in keys:
                for key in ['base','aux']:
                    bag=d['signatures'][int(row[key])];s.write(struct.pack('<I',len(bag)));s.write(hashes[bag].astype('<u8').tobytes())
                s.write(row['tail'].astype('<f2').tobytes())
            s.write(w.astype('<f2').tobytes());s.write(np.array([b],'<f4').tobytes())
    np.concatenate(refs).astype('<f4').tofile(out/'reference.bin')
    subprocess.run([str(worker.resolve()),str(out/'input.bin'),str(out/'reference.bin'),str(out/'gpu.json')],check=True,stdout=(out/'gpu.log').open('w'))
    gpu=json.loads((out/'gpu.json').read_text());assert gpu['threshold_disagreements']==0
    final={'schema':'polyspine-affine-results/v1','scope':'offline prediction on same native Pass 3 dataset, not runtime acceleration','dataset_sha256':sha(dataset/'dataset.json'),
           'capture_sha256':sha(capture/'capture.json'),'arithmetic_mode':'audit: exact integer DAG validates all tuples; safe-range pooled outputs read GEMM columns, other tuples use i64 outputs','fixed_coefficient_source':'native typed terms RHS, not learned','results':results,'gpu':gpu,'jobs':jobs,
           'arithmetic_audit':json.loads((prepared/'audit.json').read_text()),'arithmetic_gpu':json.loads((prepared/'gpu/gpu.json').read_text()),
           'sources':{name:sha(Path(__file__).with_name(name)) for name in ['affine_fit.py','affine_capture.py','affine_terms.py','affine.metal','affine_worker.swift']},
           'old_model_packet_sha256':sha(oldfit/'input.bin'),'threshold':.5,'ridge_lambda':1,'width':512,'new_feature_columns':NUMERIC.tolist(),'feature_transform':'signed log2(1+abs(i64))/64, correlated-tuple exact outputs then root reductions',
           'limits':['CPU builds PRE-stage correlated contexts; this cost has not been eliminated.','Exact smaller-cost guards do not certify the broad observable-update label.','No future labels or future alias representatives enter features.','Fixed score-plus-arithmetic fusion leaves the old score unchanged; learned use of guard outputs is a subsequent step.']}
    final['diagnosis']={}
    for split,train in masks.items():
        a=np.load(out/f'{split}-baseline.npz');b=np.load(out/f'{split}-affine_guard.npz')
        truth=a['truth'];prior=a['score']>=.5;pred=b['score']>=.5
        final['diagnosis'][split]={'newly_recalled':int(sum(truth&~prior&pred)),'lost_recalled':int(sum(truth&prior&~pred)),
            'remaining_fn_incomplete':int(sum(truth&~pred&~b['complete'])),'remaining_fn_complete':int(sum(truth&~pred&b['complete'])),
            'positive_with_old_cost_decrease':int(sum(truth&ci[~train])),'positive_without_old_cost_decrease':int(sum(truth&~ci[~train]))}
    (out/'results.json').write_text(json.dumps(final,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('mode',choices=['prepare','fit'])
    for n in ['dataset','capture','oldfit','output','worker']:p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--prepared',type=Path);a=p.parse_args()
    if a.mode=='prepare':prepare(a.dataset,a.capture,a.oldfit,a.output,a.worker)
    else:fit(a.dataset,a.capture,a.oldfit,a.prepared,a.output,a.worker)
