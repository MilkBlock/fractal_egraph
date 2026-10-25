import argparse
import hashlib
import json
from pathlib import Path
import time
import numpy as np
import train
from audit import setup,diagnose
from context import footprints,augment


def separate_blocks(left,right):
    result={}
    for t,a in left.items():
        b=right[t];pairs={};values=[]
        def intern(i,j):
            pair=(int(i),int(j))
            if pair not in pairs:pairs[pair]=len(values);values.append(pair)
            return pairs[pair]
        domain=np.array([intern(i,j) for i,j in zip(a['domain'],b['domain'],strict=True)],dtype=np.int64)
        query=np.array([intern(i,j) for i,j in zip(a['query'],b['query'],strict=True)],dtype=np.int64)
        result[t]={'domain':domain,'query':query,
            'x':np.stack([np.concatenate([a['x'][i],b['x'][j]]) for i,j in values]),
            'block_hashes':[(a['hashes'][i],b['hashes'][j]) for i,j in values],
            'signature_hash':np.array([train.fingerprint([int(a['signature_hash'][i]),int(b['signature_hash'][j])]) for i,j in values],dtype=np.uint64)}
    return result


def preserve_local_columns(coords,model,local_model,mask):
    width=local_model['w'].shape[0]
    if model['w'].shape[0]!=2*width:raise ValueError('expected two coordinate blocks')
    model['w'][:width,mask]=local_model['w'][:,mask]*2
    model['w'][width:,mask]=0;model['bias'][mask]=local_model['bias'][mask]
    model['scores']=coords['x'].astype(np.float32)@model['w'].astype(np.float32)/(2*width)+model['bias']
    return model['scores'][coords['query']]


def main():
    p=argparse.ArgumentParser()
    for n in ('snapshot','profile','source','analyze-queue','native-report','output'):p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--typed-mask-only',action='store_true')
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    bank,groups,routes,allowed,vocab,graph=setup(a.snapshot,a.profile,a.source,a.analyze_queue)
    oracle=json.loads(a.native_report.read_text())
    digest=hashlib.sha256(str(sorted((int(q),int(r)) for q,r in bank.answers())).encode()).hexdigest()
    if digest!=oracle['root_projection_sha256']:raise ValueError('native root projection mismatch')
    for key,path in [('snapshot_sha256',a.snapshot),('profile_sha256',a.profile)]:
        if oracle[key]!=hashlib.sha256(path.read_bytes()).hexdigest():raise ValueError('native input mismatch')
    start=time.perf_counter();base=graph.tokens(2,vocab)
    local,recursive,work=footprints(graph);setup_seconds=time.perf_counter()-start
    report={'scope':'constructor-only contextual features; not native rule execution or live Cell state',
        'source_hashes':{k:hashlib.sha256(p.read_bytes()).hexdigest() for k,p in
            [('snapshot',a.snapshot),('profile',a.profile),('source',a.source),('analyze_queue',a.analyze_queue)]},
        'root_projection_sha256':digest,'shared_feature_seconds':setup_seconds,'footprint_work':work,'trials':[]}
    local_coords=None
    variants=[('typed_mask',recursive)] if a.typed_mask_only else [('local',local),('recursive',recursive),('split_recursive',recursive)]
    for variant,extra in variants:
        start=time.perf_counter()
        if variant in ('split_recursive','typed_mask'):
            if variant=='typed_mask':local_coords=train.coordinates(groups,base)
            right=train.coordinates(groups,{v:{('context',*t) for t in ts} for v,ts in recursive.items()})
            coords=separate_blocks(local_coords,right)
        else:
            coords=train.coordinates(groups,augment(base,extra))
            if variant=='local':local_coords=coords
        encode_seconds=time.perf_counter()-start
        splits=('new_binding','new_binding_fresh','new_binding_third') if a.typed_mask_only else ('new_binding','new_binding_fresh')
        for split in splits:
            start=time.perf_counter();predictions={};scope={};models={};held_count=0;baseline={}
            for t,g in groups.items():
                held,scope[t]=train.heldout_split(g,coords[t],split);held_count+=int(held.sum())
                score,prior,models[t]=train.fit(g,coords[t],held);predictions[t]=(score,prior)
                if variant=='typed_mask':
                    local_score,local_prior,lm=train.fit(g,local_coords[t],held);baseline[t]=(local_score,local_prior)
                    mask=[i for i,index in enumerate(g['indices']) if index[0] in allowed]
                    score=preserve_local_columns(coords[t],models[t],lm,mask)
                    if not np.array_equal(score[:,mask]>=.5,local_score[:,mask]>=.5):raise AssertionError('constructor mask changed local decisions')
                    predictions[t]=(score,prior)
            record={'variant':variant,'split':split,'held_out_keys':held_count,'coordinate_seconds':encode_seconds,
                'fit_predict_seconds':time.perf_counter()-start,
                'model_bytes':sum(m['w'].nbytes+m['bias'].nbytes for m in models.values()),
                'signature_count':sum(len(c['x']) for c in coords.values()),
                'dependent_pairs':train.memory_run.assess(bank,groups,routes,predictions,.5,query_scope=scope),
                'non_constructor_channels':train.condition_subset(groups,predictions,scope,allowed)}
            if variant=='typed_mask':record['paired_baseline']=train.memory_run.assess(bank,groups,routes,baseline,.5,query_scope=scope)
            report['trials'].append(record);print(json.dumps(record),flush=True)
            if variant=='recursive' and split=='new_binding':
                report['gpu_jobs']=train.write_gpu(a.output/'input.bin',groups,coords,models)
                (a.output/'recursive-audit.json').write_text(json.dumps(diagnose(bank,groups,routes,coords,predictions,scope),indent=2)+'\n')
            if variant=='typed_mask':
                (a.output/(split+'-audit.json')).write_text(json.dumps(diagnose(bank,groups,routes,coords,predictions,scope),indent=2)+'\n')
                if split=='new_binding':report['gpu_jobs']=train.write_gpu(a.output/'input.bin',groups,coords,models)
    report['limits']=['Topology is current frozen-graph input; held-out keys still have constructor structure.',
        'Only same-sort operands propagate. Cross-sort containers and scope interactions are not assumed equivalent.',
        'Presence of an operator somewhere in a descendant alternative does not prove a semantic effect or impurity.',
        'All tag propagation is on CPU in this prototype; incremental deletion/visibility/union invalidation is not implemented.',
        'The existing development split and the additional paired split are both from the same graph.']
    (a.output/'results.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__':main()
