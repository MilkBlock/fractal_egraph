"""Ridge on GPU-projected residual context, with matched pending-only controls."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import time
import numpy as np
from residual import project
from study import codes, fingerprint, fit_model, counts, sha

VARIANTS=['shape_pending','state_1','state_2','state_4','state_8','erased_8','shuffled_8']


def evaluate(dataset, projection, out, worker, coordinates='explicit'):
    out.mkdir(parents=True,exist_ok=False)
    capture=json.loads((dataset/'capture.json').read_text());old_dir=Path(capture['old_dataset'])
    old=json.loads((old_dir/'dataset.json').read_text());gpu=json.loads((projection/'projection.json').read_text())
    for f in capture['frames']:
        if sha(dataset/f['packet'])!=f['packet_sha256'] or sha(dataset/f['data'])!=f['data_sha256']:
            raise ValueError('capture artifact changed')
    vocab=[];token_ids={};sigs=[];sig_ids={};frames=[];checks=[]
    def intern(bags,tail):
        key=[]
        for bag in bags:
            ids=[]
            for t in sorted(bag):
                if t not in token_ids:token_ids[t]=len(vocab);vocab.append(t)
                ids.append(token_ids[t])
            key.append(tuple(ids))
        key=tuple(key)+(tuple(tail),)
        if key not in sig_ids:sig_ids[key]=len(sigs);sigs.append(key)
        return sig_ids[key]
    old_token=[json.dumps(t,separators=(',',':')) for t in old['vocabulary']]
    start=time.perf_counter()
    for f in capture['frames']:
        z=np.load(dataset/f['data']);pending=z['before']==0;cycle=f['cycle']
        p=np.fromfile(projection/f'cycle-{cycle:02}-prestate.bin',dtype='<f4').reshape(-1,17)
        erased=np.fromfile(projection/f'cycle-{cycle:02}-erased_prestate.bin',dtype='<f4').reshape(-1,17)
        np.testing.assert_array_equal(p[:,[0,1,3,5,7,9,11,13,15]],z['projection'])
        # Independently check the erased control, not only the admitted one.
        c=dict(np.load(dataset/f'cycle-{cycle:02}-context.npz'))
        c['state_width']=f['coordinates']//len(capture['program']['family'])
        writes={r['output'][0] for r in capture['program']['recipes']}
        for i,pred in enumerate(capture['program']['family']):
            if pred in writes:c['seed'][i*c['state_width']:(i+1)*c['state_width']]=0
        np.testing.assert_array_equal(erased[:,[0,1,3,5,7,9,11,13,15]],project(c))
        checks.append({'cycle':cycle,'proof_coordinates_exact':True,'erased_coordinates_exact':True})
        ids=np.zeros((len(z['y']),len(VARIANTS)),np.uint32)
        perm=np.random.default_rng(1831+cycle).permutation(len(ids))
        for j in range(len(ids)):
            base,local=[{old_token[t] for t in bag} for bag in old['signatures'][int(z['old_signatures'][j,3])]]
            for v,name in enumerate(VARIANTS):
                extra=set();depth=0 if name=='shape_pending' else int(name.rsplit('_',1)[1])
                row=erased[j] if name=='erased_8' else p[perm[j]] if name=='shuffled_8' else p[j]
                tail=np.zeros(32,np.float16)
                # Root prior truth is deliberately omitted; all learned samples
                # are pending. Only connected premise-readiness is appended.
                for k in range(1,depth+1):
                    if coordinates=='explicit':tail[2*k-2:2*k]=row[2*k-1:2*k+1]
                    else:
                        extra.add(json.dumps(['residual-proof',k,int(row[2*k-1])],separators=(',',':')))
                        extra.add(json.dumps(['residual-ready',k,float(np.float16(row[2*k]))],separators=(',',':')))
                ids[j,v]=intern([base,local|extra],tail)
        frames.append({'cycle':cycle,'ids':ids,'y':z['y'],'pending':pending,'projection':p,'frozen':z['frozen'],
                       'keys':z['keys'],'old_frame':f['old_frame']})
    packing=time.perf_counter()-start
    hashes=np.array([fingerprint(t) for t in vocab],np.uint64);tc=codes(hashes,256)
    x=np.zeros((len(sigs),512),np.float16)
    for i,bags in enumerate(sigs):
        for b,bag in enumerate(bags[:2]):
            div=1
            while div*div<len(bag):div*=2
            if bag:x[i,b*256:(b+1)*256]=tc[list(bag)].sum(0)/div
        if coordinates=='explicit':x[i,-32:]=bags[2]
    test_y=np.concatenate([f['y'][f['pending']] for f in frames if f['cycle']>=6]);results=[];models=[];scores={}
    for v,name in enumerate(VARIANTS):
        started=time.perf_counter();n=np.zeros(len(sigs),np.int64);pos=n.copy()
        for f in frames:
            if f['cycle']>=6:continue
            ids=f['ids'][f['pending'],v];np.add.at(n,ids,1);np.add.at(pos,ids,f['y'][f['pending']])
        w,b,prior=fit_model(x,n,pos);allscore=x.astype(np.float32)@w.astype(np.float32)/512+b
        score=np.concatenate([allscore[f['ids'][f['pending'],v]] for f in frames if f['cycle']>=6])
        metrics=counts(test_y,score);scores[name]=score;models.append((w,b,allscore))
        result={'variant':name,'train_rows':int(n.sum()),'train_positive':int(pos.sum()),'test':metrics,
                'fit_and_score_seconds':time.perf_counter()-started,'prior':prior}
        results.append(result);print(name,metrics,flush=True)
    deterministic={}
    for k in [1,2,4,8]:
        s=np.concatenate([f['projection'][f['pending'],2*k-1] for f in frames if f['cycle']>=6])
        deterministic[str(k)]=counts(test_y,s)
    frozen=np.concatenate([f['frozen'][f['pending']] for f in frames if f['cycle']>=6]).astype(np.float32)
    # Persist all missed/false positive pending keys for concrete case analysis.
    errors=[]
    for f in frames:
        if f['cycle']<6:continue
        truth=f['y'].astype(bool);proof=f['projection'][:,15]>0
        for j in np.flatnonzero(f['pending']&(truth!=proof)):
            errors.append({'cycle':f['cycle'],'row':int(j),'key':str(f['keys'][j]),'truth':bool(truth[j]),
                           'proof':bool(proof[j]),'old_frame':f['old_frame']})
    refs=[]
    with (out/'input.bin').open('wb') as stream:
        stream.write(b'SPINE004' if coordinates=='explicit' else b'SPINE003');stream.write(struct.pack('<I',len(VARIANTS)))
        for v,name in enumerate(VARIANTS):
            used=np.unique(np.concatenate([f['ids'][f['pending'],v] for f in frames if f['cycle']>=6]))
            w,b,allscore=models[v];stream.write(struct.pack('<3I',len(used),512,1))
            for i in used:
                for bag in sigs[i][:2]:
                    stream.write(struct.pack('<I',len(bag)));stream.write(hashes[list(bag)].astype('<u8').tobytes())
                if coordinates=='explicit':stream.write(np.array(sigs[i][2],'<f2').tobytes())
            stream.write(w.astype('<f2').tobytes());stream.write(np.array([b],'<f4').tobytes());refs.append(allscore[used])
    np.concatenate(refs).astype('<f4').tofile(out/'reference.bin')
    with (out/'score.log').open('w') as log:
        subprocess.run([str(worker.resolve()),str(out/'input.bin'),str(out/'reference.bin'),str(out/'score.json')],check=True,stdout=log,stderr=log)
    score_gpu=json.loads((out/'score.json').read_text())
    if score_gpu['threshold_disagreements']:raise AssertionError('GPU threshold differs')
    totalrows=sum(f['query_rows'] for f in capture['frames']);inc=sum(f['incidences'] for f in capture['frames'])
    report={'schema':'polyspine-residual-results/v1','scope':'offline pending-answer prediction with device-derived finite-depth residual context',
            'source_sha256':capture['source_sha256'],'capture':str(dataset.resolve()),'capture_sha256':sha(dataset/'capture.json'),
            'old_dataset_sha256':capture['old_dataset_sha256'],'native_runs':capture['native_runs'],'trace_exact':capture['trace_exact'],
            'split':{'train_cycles':list(range(6)),'test_cycles':list(range(6,12)),'same_program_and_possible_repeated_bindings':True},
            'coordinates':coordinates,'selected_rules':capture['program']['selected_rules'],'admitted_rules':len(capture['program']['recipes']),
            'rejected':capture['program']['rejected'],'results':results,'deterministic_projection':deterministic,
            'frozen_closure':counts(test_y,frozen),'projection_errors':errors,'gpu_checks':checks,'score_gpu':score_gpu,
            'projection_gpu_seconds':sum(f['gpu_seconds'] for f in gpu['frames'] if f['mode']=='prestate'),
            'erased_projection_gpu_seconds':sum(f['gpu_seconds'] for f in gpu['frames'] if f['mode']=='erased_prestate'),
            'offline_observation_seconds':sum(f['observation_seconds'] for f in capture['frames']),
            'offline_packing_seconds':sum(f['packing_seconds'] for f in capture['frames']),
            'offline_diagnostic_seconds':sum(f['diagnostic_seconds'] for f in capture['frames']),
            'offline_fit_feature_packing_seconds':packing,'total_observed_query_rows':totalrows,
            'incidences_per_rule_row':inc/sum(f['clauses'] for f in capture['frames']),
            'projection_arithmetic':'sparse fixed column gather + AND + OR; NOT GEMM or learned inference',
            'score_arithmetic':'FP16/FP32 ridge GEMM; not a correctness proof',
            'limits':capture['limits']+['No whole-engine speedup; unsupported paths still need exact completion.',
                                      'Sparse admitted dependencies do not prove arbitrary egraphs are sparse.',
                                      'A projected positive is a frozen-carrier diagnostic, not a production closure certificate.'],
            'score_worker_sha256':sha(worker),
            'sources':{p.name:sha(p) for p in [Path(__file__),Path(__file__).with_name('residual.py'),Path(__file__).with_name('residual.metal'),Path(__file__).with_name('residual_worker.swift')]}}
    (out/'results.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--dataset',type=Path,required=True);p.add_argument('--projection',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--worker',type=Path,required=True)
    p.add_argument('--coordinates',choices=['tokens','explicit'],default='explicit')
    a=p.parse_args();evaluate(a.dataset,a.projection,a.output,a.worker,a.coordinates)
