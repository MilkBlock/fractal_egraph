"""Audit and fit the existing Polyspine ridge procedure on resident contexts.

Only explicit training captures transfer matrices. Ordinary inference loads a
small model once and keeps contexts, exact effects and scores on the GPU.
"""
import argparse,hashlib,json,struct,sys
from pathlib import Path
import numpy as np
HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'polyspine_ridge'))
from study import fit_model,counts,codes,fingerprint

def read_arrays(raw,at,specs):
    result=[]
    for dtype,shape in specs:
        n=int(np.prod(shape));dt=np.dtype(dtype);result.append(np.frombuffer(raw,dt,n,at).reshape(shape).copy());at+=n*dt.itemsize
    if at!=len(raw):raise ValueError('packet length differs from declared dimensions')
    return result

def frame(path):
    b=path.read_bytes()
    if b[:8]!=b'JPRDG001':raise ValueError('frame schema')
    n,f,w,phase,epoch,scope=struct.unpack_from('<6I',b,8)
    if w!=136 or phase>2:raise ValueError('frame shape')
    roots,base,h,scores,before,after=read_arrays(b,32,[('<u4',(n,)),('<f2',(n,f)),('<f2',(n,w)),('<f4',(n,8)),('<u4',(n,8)),('<u4',(n,8))])
    if np.any(after[:,1:5]<before[:,1:5]):raise ValueError('effect generation went backwards')
    delta=after.astype(np.int64)-before.astype(np.int64)
    y=np.column_stack([delta[:,1]>0,(delta[:,2]>0)|(delta[:,4]>0),delta[:,3]>0])
    return dict(path=path,roots=roots,base=base,h=h,scores=scores,before=before,after=after,y=y,phase=phase,epoch=epoch,scope=scope)

def auxiliary(path):
    b=path.read_bytes()
    if b[:8]!=b'JPSPN001':raise ValueError('Interface packet schema')
    n,m,f,c=struct.unpack_from('<4I',b,8)
    values=read_arrays(b,24,[('<u4',(n,)),('<u4',(m,8)),('<i4',(n,64)),('<u4',(n,)),('<u4',(n+1,)),('<u4',(m,)),('<i8',(m,c)),('<u4',(m,))])
    return dict(zip(['roots','markers','heads','head_counts','offsets','items','payload','tables'],values))

def load(run):
    parent=run/'polyspine-training';manifest=json.loads((parent/'manifest.json').read_text());full=json.loads((run/'program.json').read_text());p=full['joint_polyspine']['program']
    if p['layout_sha256']!=manifest['layout_sha256']:raise ValueError('capture/compiler layout mismatch')
    paths=sorted(parent.glob('scope-*-phase-*.bin'),key=lambda p:tuple(map(int,[p.stem.split('-')[1],p.stem.split('-')[3]])))
    if not paths:raise ValueError('no accepted joint scopes were captured')
    return p,[frame(x) for x in paths]

def audit(run,output=None):
    p,frames=load(run);folder=run/'polyspine-training';results=[];cache={}
    expected=codes(np.array([fingerprint(('resident-feature',x)) for x in p['feature_layout']['features']],np.uint64),64)
    np.testing.assert_array_equal(expected,np.asarray(p['encoding']))
    e=np.asarray(p['encoding'],np.float32);roles=np.asarray(p['role_encoding'],np.float32);rotation=p['role_rotation'];model=p['model']
    for z in frames:
        if z['scope'] not in cache:cache[z['scope']]=auxiliary(folder/f"scope-{z['scope']}-interfaces.bin")
        a=cache[z['scope']];np.testing.assert_array_equal(a['roots'],z['roots']);root={int(k):i for i,k in enumerate(a['roots'])}
        if len(root)!=len(a['roots']) or len(set(map(int,a['markers'][:,7])))!=len(a['markers']):raise AssertionError('lost Interface occurrence identity')
        for mi,(marker,values,table) in enumerate(zip(a['markers'],a['payload'],a['tables'])):
            r=p['recipes'][int(marker[1])]
            assert int(table)==r['table'] and int(marker[2])==int(values[r['root_column']])
            for j,port in enumerate(r['inputs']):assert int(marker[4+j])==int(values[port['column']])
        encoded=z['base'].astype(np.float32)@e/np.sqrt(np.float32(e.shape[0]));seam=np.zeros_like(encoded)
        for i in range(len(a['roots'])):
            value=a['heads'][i].astype(np.float32)/max(1,int(a['head_counts'][i]));n=1
            for at in range(int(a['offsets'][i]),int(a['offsets'][i+1])):
                marker=a['markers'][a['items'][at]];assert root[int(marker[2])]==i
                for j in range(int(marker[3])):
                    child=root[int(marker[4+j])];role=int(marker[1])*3+j;indices=(np.arange(64)+rotation[role])%64
                    v=encoded[child,indices]+a['heads'][child,indices].astype(np.float32)/max(1,int(a['head_counts'][child]))
                    value=value+roles[role]*v;n+=1
            seam[i]=value/n
        expected=np.column_stack([encoded,seam,np.ones((len(encoded),1)),np.zeros((len(encoded),7))]).astype(np.float16)
        error=float(np.max(np.abs(expected.astype(np.float32)-z['h'].astype(np.float32)),initial=0))
        np.testing.assert_allclose(z['h'].astype(np.float32),expected.astype(np.float32),rtol=.002,atol=.002)
        # Independent checks of binding-derived feature fields. These are
        # prediction features, not a replacement for exact native graph checks.
        spine_counts=np.diff(a['offsets']);np.testing.assert_allclose(z['base'][:,10],np.minimum(1,np.log2(1+spine_counts)/16),atol=.0005)
        consumers=np.zeros(len(root),np.uint32)
        for marker in a['markers']:
            for child in marker[4:4+int(marker[3])]:consumers[root[int(child)]]+=1
        np.testing.assert_allclose(z['base'][:,12],np.minimum(1,np.log2(1+consumers)/16),atol=.0005)
        score_error=None;decisions=None
        if model is not None:
            w=np.asarray(model['weights'],np.float16).astype(np.float32).reshape(136,8)
            ref=(z['h'].astype(np.float32)@w)[:,:3]*np.asarray(model['scales'],np.float32)+np.asarray(model.get('bias',[0,0,0]),np.float32)
            score_error=float(np.max(np.abs(ref-z['scores'][:,:3]),initial=0));np.testing.assert_allclose(z['scores'][:,:3],ref,rtol=.002,atol=.002)
            decisions=[counts(z['y'][:,i],z['scores'][:,i]) for i in range(3)]
        results.append({'scope':z['scope'],'phase':z['phase'],'pre_phase_epoch':z['epoch'],'rows':len(z['roots']),'projection_max_error':error,'ridge_max_error':score_error,'gpu_predictions':decisions})
    result={'schema':'resident-polyspine-audit/v1','layout_sha256':p['layout_sha256'],'codebook_exact':True,'frames':results,
            'scope':'GPU coordinate/projection/score audit with retained correlated Interface records; native graph checks are separate',
            'causal_boundary':'all features captured before the selected phase; labels subtract then-current counters from joint-exit counters'}
    if output:output.write_text(json.dumps(result,indent=2)+'\n')
    print('audited',len(results),'frames');return result

def fit(run,out,train_scopes,verification=None):
    if out.exists():raise ValueError('model exists; preserve the previous artifact')
    p,frames=load(run);report=json.loads((run/'report.json').read_text())
    verified=bool(report['checks'] and report['checks'][-1].get('exact_rows_values_equivalence_visibility'))
    if verification:
        check=json.loads(verification.read_text());verified=bool(check['final_check'].get('exact_rows_values_equivalence_visibility')) and Path(check['after']).resolve()==run.resolve()
    if not verified:raise ValueError('fit requires an exact native or stored device endpoint check')
    audit(run)
    train=[f for f in frames if f['scope']<train_scopes];test=[f for f in frames if f['scope']>=train_scopes]
    if not train:raise ValueError('empty chronological training split')
    x=np.concatenate([f['h'][:,:128] for f in train]);y=np.concatenate([f['y'] for f in train])
    unique,inv=np.unique(x,axis=0,return_inverse=True);n=np.bincount(inv)
    weights=np.zeros((136,8),np.float16);bias=[]
    for head in range(3):
        pos=np.bincount(inv,weights=y[:,head]);w,b,_=fit_model(unique,n,pos,regularization=1.0);weights[:128,head]=w;bias.append(float(b))
    result={'schema':'resident-polyspine-ridge/v1','layout_sha256':p['layout_sha256'],'weights':weights.astype(float).ravel().tolist(),'scales':[1/128]*3,'bias':bias,
            'training':{'procedure':'polyspine_ridge.study.fit_model, lambda=1; exact duplicate half-vector aggregation',
                        'run':str(run.resolve()),'source_sha256':report['source_sha256'],'program_sha256':report['program_sha256'],
                        'train_scopes':train_scopes,'train_rows':len(x),'unique_feature_vectors':len(unique),'held_out_rows':sum(len(f['roots']) for f in test),
                        'targets':['remaining terms constructor effects','remaining helper rows/function updates','remaining comparison effects'],
                        'limits':'same-program chronological split; no claim of unseen-program generalization or factual authority for predictions'}}
    metrics={}
    for name,fs in [('train',train),('held_out',test)]:
        if not fs:metrics[name]=None;continue
        xx=np.concatenate([f['h'] for f in fs]).astype(np.float32);yy=np.concatenate([f['y'] for f in fs]);scores=(xx@weights.astype(np.float32))[:,:3]/128+np.asarray(bias,np.float32)
        metrics[name]=[counts(yy[:,i],scores[:,i]) for i in range(3)]
    result['metrics']=metrics;out.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({'training':result['training'],'metrics':metrics},indent=2));return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('mode',choices=['audit','fit']);p.add_argument('--run',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--train-scopes',type=int,default=1);p.add_argument('--verification',type=Path);a=p.parse_args()
    if a.mode=='audit':audit(a.run,a.output)
    else:fit(a.run,a.output,a.train_scopes,a.verification)
