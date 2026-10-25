"""Evaluate speculative key memory against the unchanged native-verified bank."""
import argparse
from collections import defaultdict
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys
import time
import numpy as np
from memory import Memory,fingerprint,shards,condition_counts,interpolation_parts

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'query_response_operator'))
from model import ResponseBank
spec=importlib.util.spec_from_file_location('response_run',HERE.parent/'query_response_operator/run.py')
response_run=importlib.util.module_from_spec(spec);spec.loader.exec_module(response_run)


def schema(index):
    table,sorts,cols,keep=index;local={}
    for (kind,slot),sort in zip(cols,sorts,strict=True):
        if kind=='slot':local[slot]=sort
    return tuple(local[x] for x in keep)


def prepare(bank):
    groups={};which={}
    for index,values in bank.indices.items():
        types=schema(index)
        if not types:continue # Scalar existence has no key to compress.
        g=groups.setdefault(types,{'indices':[],'domains':set(),'requests':set()})
        which[index]=(types,len(g['indices']));g['indices'].append(index);g['domains'].update(values)
    # Unary domain keys come from ALL raw facts, not from queried membership
    # answers. This allows meaningful priors over known typed graph identities.
    unary={types[0]:g for types,g in groups.items() if len(types)==1}
    for rows in bank.tables.values():
        for row in rows:
            for v in row:
                if v[1] in unary:unary[v[1]]['domains'].add((v,))
    routes={};demand_occurrences=0
    for name,g in bank.groups.items():
        rows=bank.tables.get(name,[])
        for feature,col in g['features'].items():
            if feature[0]!='lookup' or feature[1] not in which:continue
            _,index,positions=feature;types,channel=which[index]
            keys=[tuple(row[p] for p in positions) for row in rows]
            groups[types]['requests'].update(keys);routes[name,col]=(types,channel,keys)
            demand_occurrences+=len(keys)
    out={}
    for types,g in groups.items():
        domain=sorted(g['domains']);queries=sorted(g['requests']);c=len(g['indices'])
        if not queries:continue
        positions={key:i for i,key in enumerate(domain)}
        payload=np.zeros((len(domain),c),dtype=np.uint8)
        for j,index in enumerate(g['indices']):
            for key in bank.indices[index]:payload[positions[key],j]=1
        fingerprints=np.array([fingerprint(k) for k in domain],dtype=np.uint64)
        probes=np.array([fingerprint(k) for k in queries],dtype=np.uint64)
        out[types]={'keys':fingerprints,'payload':payload,'probes':probes,
            'query_ids':{key:i for i,key in enumerate(queries)},'indices':g['indices'],
            'keys_outside_domain':sum(key not in positions for key in queries)}
    for key,(types,c,queries) in list(routes.items()):
        if types not in out:del routes[key];continue
        routes[key]=(types,c,np.array([out[types]['query_ids'][q] for q in queries],dtype=np.int64))
    return out,routes,demand_occurrences


def assess(bank,groups,routes,predictions,threshold,prior_only=False,query_scope=None):
    total={};feature_stats={};decisions=[]
    for name,g in bank.groups.items():
        # Anchor constants/aliases and scalar existence stay exact. They do not
        # perform the keyed reads being replaced in this experiment.
        features=g['x'].astype(bool).copy()
        affected=np.zeros_like(features) if query_scope is not None else None
        for col in range(features.shape[1]):
            if (name,col) not in routes:continue
            types,c,ids=routes[name,col];scores=predictions[types][1 if prior_only else 0]
            features[:,col]=scores[ids,c]>=threshold
            if affected is not None:affected[:,col]=query_scope[types][ids]
            expected=g['x'][:,col].astype(bool)
            for k,v in condition_counts(expected,features[:,col]).items():
                if isinstance(v,int):feature_stats[k]=feature_stats.get(k,0)+v
        predicted=np.ones_like(g['answer'],dtype=bool)
        include=np.ones_like(predicted) if affected is None else np.zeros_like(predicted)
        for q,need in enumerate(g['requirements']):predicted[:,q]=features[:,sorted(need)].all(1)
        if affected is not None:
            for q,need in enumerate(g['requirements']):include[:,q]=affected[:,sorted(need)].any(1)
        decisions.append((g['answer'][include],predicted[include]))
    truth=np.concatenate([x.ravel() for x,y in decisions]);guess=np.concatenate([y.ravel() for x,y in decisions])
    total=condition_counts(truth,guess)
    total['cpu_candidate_validations']=total['proposed']
    total['warning']='missed_true is measured with the oracle; runtime cannot know these entries without its complement/fallback process'
    return {'rule_root_candidates':total,'key_condition_occurrences':feature_stats}


def write_packet(path,groups,memories):
    jobs=[]
    for types,g in groups.items():
        m=memories[types];partition=shards(g['probes'],m.parts)
        for s in range(m.parts):
            take=np.flatnonzero(partition==s)
            if len(take):jobs.append((g['probes'][take],m.weights[s],m.prior[s],types,s,take))
    with path.open('wb') as f:
        f.write(b'SKETCH01');f.write(struct.pack('<I',len(jobs)))
        for keys,w,prior,*rest in jobs:
            f.write(struct.pack('<3I',len(keys),w.shape[0],w.shape[1]))
            f.write(keys.astype('<u8').tobytes());f.write(w.astype('<f2').tobytes());f.write(prior.astype('<f4').tobytes())
    return jobs


def main():
    p=argparse.ArgumentParser();p.add_argument('--snapshot',required=True,type=Path)
    p.add_argument('--profile',required=True,type=Path);p.add_argument('--output',required=True,type=Path)
    p.add_argument('--native-report',required=True,type=Path);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    tables,header=response_run.load(a.snapshot,a.profile);bank=ResponseBank(header['queries'],tables).encode()
    previous=json.loads(a.native_report.read_text())
    for key,path in [('snapshot_sha256',a.snapshot),('profile_sha256',a.profile)]:
        if previous[key]!=hashlib.sha256(path.read_bytes()).hexdigest():raise ValueError('oracle source mismatch')
    digest=hashlib.sha256(str(sorted((int(q),int(r)) for q,r in bank.answers())).encode()).hexdigest()
    if digest!=previous['root_projection_sha256']:raise ValueError('oracle response mismatch')
    groups,routes,occurrences=prepare(bank)
    result={'scope':'lossy proposal memory for the same native-verified anchor-complete query class; no live runtime speedup',
        'native_report':str(a.native_report),'root_projection_sha256':digest,
        'snapshot_sha256':previous['snapshot_sha256'],'profile_sha256':previous['profile_sha256'],
        'condition_occurrences':occurrences,'key_schema_groups':len(groups),'configs':[],
        'groups':[{'schema':list(t),'channels':g['payload'].shape[1],'domain_keys':len(g['keys']),
            'query_keys':len(g['probes']),'query_keys_outside_domain':g['keys_outside_domain']} for t,g in groups.items()]}
    # Fixed, declared experiment grid; retain every outcome, including priors.
    for width,parts,fit in ((128,1,False),(256,8,False),(512,8,False),(256,32,False),(128,None,True),(256,None,True)):
        started=time.perf_counter()
        memories={t:Memory(g['keys'],g['payload'],width,interpolation_parts(g['keys'],width) if fit else parts,fit=fit) for t,g in groups.items()}
        predictions={t:m.predict(groups[t]['probes']) for t,m in memories.items()}
        record={'width':width,'shards':parts,'bake_decoder_correction':fit,
            'shards_by_schema':{'/'.join(t):m.parts for t,m in memories.items()},
            'coefficient_nonzero_fraction':sum(np.count_nonzero(m.weights) for m in memories.values())/sum(m.weights.size for m in memories.values()),
            'training_max_error':max(m.training_max_error for m in memories.values()) if fit else None,
            'coefficient_and_prior_bytes':sum(m.weights.nbytes+m.prior.nbytes for m in memories.values()),
            'matrix_output_coordinates':sum(len(g['probes'])*g['payload'].shape[1] for g in groups.values()),
            'prepare_and_cpu_predict_seconds':time.perf_counter()-started,'thresholds':[]}
        for threshold in (-.5,0.,.5):
            record['thresholds'].append({'threshold':threshold,
                'sketch':assess(bank,groups,routes,predictions,threshold),
                'constant_prior_only':assess(bank,groups,routes,predictions,threshold,True)})
        result['configs'].append(record)
        print(json.dumps({'width':width,'parts':parts,'bytes':record['coefficient_and_prior_bytes'],
            'quality':[{'threshold':x['threshold'],**x['sketch']['rule_root_candidates']} for x in record['thresholds']]}),flush=True)
        if width==256 and fit:
            jobs=write_packet(a.output/'input.bin',groups,memories)
            expected=np.concatenate([predictions[t][0][take].ravel() for _,_,_,t,s,take in jobs])
            expected.astype('<f4').tofile(a.output/'reference.bin')
            result['gpu_config']={'width':width,'shards':parts,'bake_decoder_correction':fit,'jobs':len(jobs),
                'scores':len(expected),'input_sha256':hashlib.sha256((a.output/'input.bin').read_bytes()).hexdigest()}
    (a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__':main()
