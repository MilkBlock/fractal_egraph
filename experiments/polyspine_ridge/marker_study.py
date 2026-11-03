"""Export native-evidence wiring, audit GPU propagation, evaluate frozen labels."""
import argparse,gzip,json,struct,subprocess,time
from pathlib import Path
import numpy as np
from affine_terms import metal_program,pair_index
from affine_fit import base_records,vector_table,encode,NUMERIC
from marker_context import compile_links,export_context,reference_step,reference_features,save_export,save_manifest
from scale_state import DeltaState
from scale_fit import KEY
from study import sha,fit_model,counts


def export(dataset,capture,workload,out):
    out.mkdir(parents=True,exist_ok=False);d=json.loads((dataset/'dataset.json').read_text());c=json.loads((capture/'capture.json').read_text());w=json.loads(workload.read_text())
    assert sha(dataset/'dataset.json')==c['dataset_sha256'] and sha(workload)==c['workload_sha256']
    assert sha(capture/'observations.jsonl.gz')==c['journal_sha256']
    program=compile_links(w['core']);assert all(r['matrix_range_certificate'] for r in program['recipes']);assert program['max_registers']<=16
    starts={f['start']:f for f in d['frames'] if f['stage']=='terms'};ends={f['end']:f for f in starts.values()};state=DeltaState();frames=[];pending={}
    with gzip.open(capture/'observations.jsonl.gz','rt') as journal:
        for line in journal:
            event=json.loads(line);state.apply(event['packet']);at=event['at']
            if at in starts:
                f=starts[at];z=np.load(dataset/f['file']);keys=[tuple(d['roots'][int(i)]) for i in z['roots']]
                begin=time.perf_counter();ctx=export_context(state.snapshot(),keys,program);seconds=time.perf_counter()-begin
                record=save_export(ctx,program,out,f['id']);record['export_seconds']=seconds
                pending[f['id']]=keys;frames.append(record);print(json.dumps(record),flush=True)
            if at in ends:
                f=ends[at];pairs,_=pair_index(state.snapshot());keys=pending.pop(f['id']);oracle=np.zeros((len(keys),2),np.int64)
                for i,k in enumerate(keys):
                    choices=pairs.get(state.lineage.find(k),[])
                    if choices:oracle[i]=[min(c for t,c in choices),1]
                np.savez_compressed(out/f"oracle-{f['id']}.npz",state=oracle)
    assert len(frames)==12 and not pending
    return save_manifest(program,frames,out,dataset_sha256=sha(dataset/'dataset.json'),journal_sha256=c['journal_sha256'],source_sha256=c['source_sha256'])


def audit(exported,gpu,out):
    out.mkdir(parents=True,exist_ok=False);d=json.loads((exported/'input.json').read_text());g=json.loads((gpu/'gpu.json').read_text());records=[]
    for f in d['frames']:
        z=np.load(exported/f['file']);c={k:z[k] for k in z.files};state=c['initial'].copy();oracle=np.load(exported/f"oracle-{f['id']}.npz")['state']
        gf=next(x for x in g['frames'] if x['id']==f['id']);checkpoints={x['depth'] for x in gf['steps']};start=time.perf_counter()
        for depth in range(1,gf['depth']+1):
            old=state;state,candidate=reference_step(c,d['program'],old)
            if depth in checkpoints:
                actual=np.fromfile(gpu/f"frame-{f['id']}-depth-{depth}-state.bin",'<i8').reshape(-1,4);np.testing.assert_array_equal(actual,state)
                actual_c=np.fromfile(gpu/f"frame-{f['id']}-depth-{depth}-candidate.bin",'<i8').reshape(-1,3);np.testing.assert_array_equal(actual_c,candidate)
                features=np.fromfile(gpu/f"frame-{f['id']}-depth-{depth}-features.bin",'<f4').reshape(-1,24);np.testing.assert_allclose(features,reference_features(c,state,candidate,old),atol=4e-6,rtol=2e-5)
                np.savez_compressed(out/f"frame-{f['id']}-depth-{depth}.npz",features=features.astype(np.float16))
        n=len(oracle);both=(oracle[:,1]>0)&(state[:n,1]>0);record={'id':f['id'],'depth':gf['depth'],'gpu_cpu_exact':True,'cpu_reference_seconds':time.perf_counter()-start,
            'native_available':int(oracle[:,1].sum()),'projected_available':int(state[:n,1].sum()),'availability_disagreements':int(sum(oracle[:,1]!=state[:n,1])),
            'minimum_cost_disagreements':int(sum(oracle[both,0]!=state[:n][both,0])),'initially_unavailable_reached':int(sum((c['initial'][:n,1]==0)&(state[:n,1]>0)))}
        np.savez_compressed(out/f"frame-{f['id']}-fixed.npz",features=features.astype(np.float16));records.append(record);print(json.dumps(record),flush=True)
    (out/'audit.json').write_text(json.dumps(records,indent=2))


def fit(dataset,projection,previous,out,worker):
    out.mkdir(parents=True,exist_ok=False);d=json.loads((dataset/'dataset.json').read_text());held=np.load(dataset/'lineage.npz')['heldout'];sc,hashes=vector_table(d)
    keys=[];labels=[];masks=[];times=[];fids=[];variants=['marker1','marker2','marker4','marker8','marker_fixed','shuffled_fixed'];features={v:[] for v in variants}
    for f,z,rec in base_records(d,dataset):
        keys.append(rec);labels.append(z['gain']);masks.append(~held[z['roots']]);times.append(np.full(len(rec),f['cycle']<6));fids.append(np.full(len(rec),f['id']))
        for variant in variants[:-1]:
            depth=variant.removeprefix('marker').removeprefix('_');suffix='fixed' if depth=='fixed' else 'depth-'+depth
            features[variant].append(np.load(projection/f"frame-{f['id']}-{suffix}.npz")['features'])
        extra=features['marker_fixed'][-1];features['shuffled_fixed'].append(extra[np.random.default_rng(4201+f['id']).permutation(len(rec))])
    rec=np.concatenate(keys);y=np.concatenate(labels);fid=np.concatenate(fids);splits={'lineage':np.concatenate(masks),'time':np.concatenate(times)};extras={k:np.concatenate(v) for k,v in features.items()}
    results=[];model_data=[];refs=[];diagnosis={}
    for split,train in splits.items():
        previous_scores=np.load(previous/f'{split}-affine_guard.npz');assert np.array_equal(previous_scores['truth'],y[~train]);oldpred=previous_scores['score']>=.5
        for variant in variants:
            r=rec.copy();r['tail'][:,NUMERIC]=extras[variant];unique,inv=np.unique(r.view(np.dtype((np.void,KEY.itemsize))).ravel(),return_inverse=True);k=unique.view(KEY);x=encode(k,sc)
            n=np.bincount(inv[train],minlength=len(k));p=np.bincount(inv[train],weights=y[train],minlength=len(k));begin=time.perf_counter();w,b,_=fit_model(x,n,p);secs=time.perf_counter()-begin
            score=x.astype(np.float32)@w.astype(np.float32)/512+b;s=score[inv];test=~train;pr=s[test]>=.5;truth=y[test];was_complete=previous_scores['complete']
            result={'split':split,'variant':variant,'test':counts(truth,s[test]),'fit_seconds':secs,
                    'old_incomplete_fn_recovered':int(sum(truth&~oldpred&~was_complete&pr)),
                    'old_complete_fn_recovered':int(sum(truth&~oldpred&was_complete&pr)),
                    'newly_recalled':int(sum(truth&~oldpred&pr)),'lost_recalled':int(sum(truth&oldpred&~pr)),
                    'subsets':{name:counts(truth[mask],s[test][mask]) for name,mask in [('old_incomplete',~was_complete),('old_complete',was_complete)]}}
            results.append(result);print(split,variant,result,flush=True)
            used=np.unique(inv[test]);model_data.append((k[used],w,b));refs.append(score[used]);np.savez_compressed(out/f'{split}-{variant}.npz',score=s[test],truth=truth,frame=fid[test])
        extra=extras['marker_fixed'][test];diagnosis[split]={'old_incomplete_fn_became_available':int(sum(truth&~oldpred&~was_complete&(extra[:,3]>0))),
            'direct_new_availability':counts(truth,extra[:,5]),'direct_new_or_better_cost':counts(truth,np.maximum(extra[:,5],extra[:,6]))}
    with (out/'input.bin').open('wb') as stream:
        stream.write(b'SPINE004');stream.write(struct.pack('<I',len(model_data)))
        for k,w,b in model_data:
            stream.write(struct.pack('<3I',len(k),512,1))
            for row in k:
                for key in ['base','aux']:
                    bag=d['signatures'][int(row[key])];stream.write(struct.pack('<I',len(bag)));stream.write(hashes[bag].astype('<u8').tobytes())
                stream.write(row['tail'].astype('<f2').tobytes())
            stream.write(w.astype('<f2').tobytes());stream.write(np.array([b],'<f4').tobytes())
    np.concatenate(refs).astype('<f4').tofile(out/'reference.bin')
    with (out/'gpu.log').open('w') as log:subprocess.run([str(worker.resolve()),str(out/'input.bin'),str(out/'reference.bin'),str(out/'gpu.json')],check=True,stdout=log)
    gpu=json.loads((out/'gpu.json').read_text());assert gpu['threshold_disagreements']==0
    result={'schema':'marker-propagation-results/v1','scope':'offline numeric projection and ridge prediction, not full Cell or runtime acceleration','dataset_sha256':sha(dataset/'dataset.json'),
            'old_results_sha256':sha(previous/'results.json'),'results':results,'diagnosis':diagnosis,'gpu':gpu,'audit':json.loads((projection/'audit.json').read_text()),
            'sources':{name:sha(Path(__file__).with_name(name)) for name in ['marker_context.py','marker_study.py','marker.metal','marker_worker.swift']},
            'limits':['Fixed PRE-stage carrier; no new Expr structures or external writes are invented.','All exported alternatives retained, but numeric projection uses their minimum, not complete candidate identities.','Marker features replace 24 existing affine feature columns at equal width; ablation is not additive.','A projection fixed point is not a native egraph saturation certificate.']}
    (out/'results.json').write_text(json.dumps(result,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('mode',choices=['export','audit','fit'])
    for name in ['dataset','capture','workload','exported','gpu','projection','previous','output','worker']:p.add_argument('--'+name,type=Path)
    a=p.parse_args()
    if a.mode=='export':export(a.dataset,a.capture,a.workload,a.output)
    elif a.mode=='audit':audit(a.exported,a.gpu,a.output)
    else:fit(a.dataset,a.projection,a.previous,a.output,a.worker)
