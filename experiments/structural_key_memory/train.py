"""Compare new-binding and unseen-shape transfer of Analyze-spine coordinates."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys
import time
import numpy as np
from features import constructors,analyze_vocabulary,native_graph,key_signature

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'superposed_key_memory'))
from memory import fingerprint,mix,codes,condition_counts
spec=importlib.util.spec_from_file_location('memory_run',HERE.parent/'superposed_key_memory/run.py')
memory_run=importlib.util.module_from_spec(spec);spec.loader.exec_module(memory_run)


def divisor(count):return 1<<(((max(1,count)-1).bit_length()+1)//2)


def encode(signatures,width=256):
    hashes=[np.array([fingerprint(token) for token in sig],dtype=np.uint64) for sig in signatures]
    x=np.stack([codes(h,width).sum(0)/divisor(len(h)) for h in hashes]).astype(np.float16)
    if not np.all(np.isfinite(x)):raise ValueError('nonfinite structural code')
    return x,hashes


def coordinates(groups,tokens):
    result={}
    for t,g in groups.items():
        ids={};sigs=[]
        def intern(key):
            sig=key_signature(key,tokens)
            if sig not in ids:ids[sig]=len(sigs);sigs.append(sig)
            return ids[sig]
        domain=np.array([intern(k) for k in g['domain_values']],dtype=np.int64)
        query=np.array([intern(k) for k in g['query_values']],dtype=np.int64)
        x,hashes=encode(sigs)
        result[t]={'x':x,'domain':domain,'query':query,'hashes':hashes,
            'signature_hash':np.array([fingerprint(sig) for sig in sigs],dtype=np.uint64),
            'token_counts':[len(sig) for sig in sigs]}
    return result


def random_coordinates(groups):
    # Matched ablation: same width, ridge solver, type grouping, labels, split
    # and threshold. Only replace structural features with opaque-ID codes.
    result={}
    for t,g in groups.items():
        values=sorted(set(g['domain_values'])|set(g['query_values']))
        ids={v:i for i,v in enumerate(values)};h=np.array([fingerprint(v) for v in values],dtype=np.uint64)
        result[t]={'x':codes(h,256).astype(np.float16),
            'domain':np.array([ids[k] for k in g['domain_values']],dtype=np.int64),
            'query':np.array([ids[k] for k in g['query_values']],dtype=np.int64),'signature_hash':h}
    return result


def fit(g,c,held):
    take=np.flatnonzero(~held);n=len(take);y=g['payload'][take].astype(np.float32)
    d=c['x'].shape[1]
    if not n:
        w=np.zeros((d,g['payload'].shape[1]),dtype=np.float16);b=np.zeros(w.shape[1],dtype=np.float32)
    else:
        # Aggregate equal structural coordinates without erasing contradictory
        # labels. Counts and sums implement frequency-weighted least squares.
        ids=c['domain'][take];count=np.bincount(ids,minlength=len(c['x'])).astype(np.float32)
        sums=np.zeros((len(c['x']),y.shape[1]),dtype=np.float32);np.add.at(sums,ids,y)
        x=c['x'].astype(np.float32);mx=count@x/n;my=y.mean(0)
        centered=x-mx
        gram=centered.T@(centered*count[:,None])+np.eye(d,dtype=np.float32)
        rhs=centered.T@(sums-count[:,None]*my)
        theta=np.linalg.solve(gram,rhs);w=(theta*d).astype(np.float16);b=(my-mx@theta).astype(np.float32)
    if not np.all(np.isfinite(w)):raise ValueError('FP16 model overflow')
    scores=c['x'].astype(np.float32)@w.astype(np.float32)/d+b
    prior=np.broadcast_to(y.mean(0) if n else b,(len(g['probes']),w.shape[1])).copy()
    population=np.bincount(c['domain'],minlength=len(c['x']))
    positives=np.zeros((len(c['x']),g['payload'].shape[1]),dtype=np.int32)
    np.add.at(positives,c['domain'],g['payload'])
    ambiguous=np.any((positives>0)&(positives<population[:,None]),axis=1)
    return scores[c['query']],prior,{'w':w,'bias':b,'scores':scores,'train_keys':n,
        'contradictory_profile_count':int(ambiguous.sum())}


def heldout_split(g,c,mode):
    salts={'new_binding':0x6a09e667f3bcc909,'new_binding_fresh':0x3c6ef372fe94f82b,
           'new_binding_third':0xa54ff53a5f1d36f1}
    if mode in salts:
        salt=np.uint64(salts[mode])
        held=(mix(g['keys']^salt)&np.uint64(7))==0
    else:
        flags=(mix(c['signature_hash']^np.uint64(0xbb67ae8584caa73b))&np.uint64(7))==0
        held=flags[c['domain']]
    scope=np.isin(g['probes'],g['keys'][held])
    return held,scope


def condition_subset(groups,predictions,scope,allowed):
    expected=[];guessed=[];prior=[]
    for t,g in groups.items():
        channels=[i for i,index in enumerate(g['indices']) if index[0] not in allowed]
        if not channels:continue
        rows=np.flatnonzero(scope[t]);lookup={k:i for i,k in enumerate(g['domain_values'])}
        for q in rows:
            row=g['payload'][lookup[g['query_values'][q]],channels].astype(bool)
            expected.extend(row);guessed.extend(predictions[t][0][q,channels]>=.5);prior.extend(predictions[t][1][q,channels]>=.5)
    e=np.array(expected,dtype=bool)
    return {'scope':'held-out unique key/channel pairs, excluding all constructor-target channels',
        'structural':condition_counts(e,np.array(guessed,dtype=bool)),
        'prior':condition_counts(e,np.array(prior,dtype=bool))}


def write_gpu(path,groups,coords,models):
    tables=[];references=[];sizes=[]
    quadratic={bool(c.get('quadratic',False)) for c in coords.values()}
    if len(quadratic)!=1:raise ValueError('mixed GPU coordinate programs')
    blocks={bool(c.get('block_hashes')) for c in coords.values()}
    if len(blocks)!=1:raise ValueError('mixed GPU block coordinate programs')
    with path.open('wb') as f:
        f.write(b'SPINE003' if True in blocks else b'SPINE002' if True in quadratic else b'SPINE001');f.write(struct.pack('<I',len(groups)))
        for t,g in groups.items():
            c=coords[t];m=models[t];used=np.unique(c['query']);n=len(used);d,channels=m['w'].shape
            f.write(struct.pack('<3I',n,d,channels))
            for row in used:
                bags=c['block_hashes'][row] if True in blocks else [c['hashes'][row]]
                for hashes in bags:f.write(struct.pack('<I',len(hashes)));f.write(hashes.astype('<u8').tobytes())
            f.write(m['w'].astype('<f2').tobytes());f.write(m['bias'].astype('<f4').tobytes())
            references.append(m['scores'][used].ravel());tables.append(list(t))
            sizes.append({'schema':list(t),'unique_query_spines':n,'probe_keys':len(c['query']),
                'weight_bytes':m['w'].nbytes+m['bias'].nbytes,'shape':[n,d,channels]})
    np.concatenate(references).astype('<f4').tofile(path.with_name('reference.bin'))
    return sizes


def main():
    p=argparse.ArgumentParser()
    for n in ('snapshot','profile','source','analyze-queue','native-report','output'):p.add_argument('--'+n,required=True,type=Path)
    p.add_argument('--parameters-only',action='store_true',help='evaluate the extra parameter variant without repeating the unchanged controls')
    p.add_argument('--random-control',action='store_true',help='same solver and split, replace structural coordinates by opaque-key codes')
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    start=time.perf_counter();tables,header=memory_run.response_run.load(a.snapshot,a.profile)
    bank=memory_run.ResponseBank(header['queries'],tables).encode();groups,routes,_=memory_run.prepare(bank)
    oracle=json.loads(a.native_report.read_text())
    digest=hashlib.sha256(str(sorted((int(q),int(r)) for q,r in bank.answers())).encode()).hexdigest()
    if digest!=oracle['root_projection_sha256']:raise ValueError('native answer identity changed')
    for field,path in [('snapshot_sha256',a.snapshot),('profile_sha256',a.profile)]:
        if hashlib.sha256(path.read_bytes()).hexdigest()!=oracle[field]:raise ValueError('oracle input changed')
    allowed=constructors(a.source);vocab,evidence=analyze_vocabulary(a.analyze_queue,allowed)
    graph=native_graph(tables,allowed)
    result={'scope':'Analyze-derived constructor spine coordinates, not live Cell identification or a closure certificate',
        'snapshot_sha256':oracle['snapshot_sha256'],'profile_sha256':oracle['profile_sha256'],
        'source_sha256':hashlib.sha256(a.source.read_bytes()).hexdigest(),'root_projection_sha256':digest,
        'analyze':evidence,'constructor_rows_scanned':len(graph.rows),'constructor_types':len(allowed),
        'setup_seconds':time.perf_counter()-start,'experiments':[]}
    if a.parameters_only and a.random_control:p.error('choose one extra variant')
    variants=[(-1,False)] if a.random_control else [(2,True)] if a.parameters_only else [(0,False),(1,False),(2,False)]
    for depth,parameters in variants:
        start=time.perf_counter()
        coords=random_coordinates(groups) if a.random_control else coordinates(groups,graph.tokens(depth,vocab,parameters))
        extraction=time.perf_counter()-start
        for mode in (('new_binding',) if a.random_control else ('new_binding','new_structure')):
            start=time.perf_counter();predictions={};scope={};models={};held_count=0
            for t,g in groups.items():
                held,scope[t]=heldout_split(g,coords[t],mode);held_count+=int(held.sum())
                scores,prior,models[t]=fit(g,coords[t],held);predictions[t]=(scores,prior)
            row={'depth':depth,'parameters':parameters,'random_control':a.random_control,'split':mode,'held_out_keys':held_count,'feature_extraction_seconds':extraction,
                'fit_and_prediction_seconds':time.perf_counter()-start,
                'all_pairs':memory_run.assess(bank,groups,routes,predictions,.5),
                'held_out_dependent_pairs':memory_run.assess(bank,groups,routes,predictions,.5,query_scope=scope),
                'prior_held_out':memory_run.assess(bank,groups,routes,predictions,.5,True,query_scope=scope),
                'non_constructor_channels':condition_subset(groups,predictions,scope,allowed),
                'structural_profiles':sum(len(c['x']) for c in coords.values()),
                'model_bytes':sum(m['w'].nbytes+m['bias'].nbytes for m in models.values()),
                'inconsistent_label_profiles':sum(m['contradictory_profile_count'] for m in models.values())}
            result['experiments'].append(row)
            print(json.dumps({k:row[k] for k in ('depth','split','held_out_keys','structural_profiles','inconsistent_label_profiles')}|
                {'quality':row['held_out_dependent_pairs']['rule_root_candidates'],'non_constructor':row['non_constructor_channels']}),flush=True)
            if depth==2 and mode=='new_binding':
                result['gpu_jobs']=write_gpu(a.output/'input.bin',groups,coords,models)
    result['limits']=['Current constructor topology is visible for held-out keys; target analysis labels are withheld. This is not unseen-program evaluation.',
        'Features use observed shared-spine shapes from Analyze, not asserted equivalence to a whole RipSatCell.',
        'Constructor-target channels can be directly exposed by constructor features; the non-constructor subset is reported separately.',
        'CPU extraction scans the graph here; online marker publishing and incremental union invalidation are not implemented.',
        'All predictors are speculative; CPU positive verification alone cannot recover false negatives.']
    (a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__':main()
