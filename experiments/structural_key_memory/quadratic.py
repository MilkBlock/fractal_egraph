"""A fixed elementwise nonlinearity, with a fresh matched holdout control.

No new graph read, target label feature, learned hidden layer, or per-key model.
The fresh split is declared before evaluating the variant; this remains a
development experiment on the same graph, not external-workload validation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import time
import numpy as np
import train
from audit import setup,diagnose


def lift(c):
    x=c['x'].astype(np.float32)
    return {**c,'x':np.concatenate([x,x*x],axis=1).astype(np.float16),'quadratic':True}


def main():
    p=argparse.ArgumentParser()
    for n in ('snapshot','profile','source','analyze-queue','native-report','output'):p.add_argument('--'+n,type=Path,required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    start=time.perf_counter();bank,groups,routes,allowed,vocab,graph=setup(a.snapshot,a.profile,a.source,a.analyze_queue)
    oracle=json.loads(a.native_report.read_text())
    digest=hashlib.sha256(str(sorted((int(q),int(r)) for q,r in bank.answers())).encode()).hexdigest()
    if digest!=oracle['root_projection_sha256']:raise ValueError('native root projection changed')
    for key,path in [('snapshot_sha256',a.snapshot),('profile_sha256',a.profile)]:
        if oracle[key]!=hashlib.sha256(path.read_bytes()).hexdigest():raise ValueError('native input mismatch')
    coords=train.coordinates(groups,graph.tokens(2,vocab));base_seconds=time.perf_counter()-start
    report={'scope':'same-graph speculative query response; no runtime or NVIDIA throughput claim',
        'source_hashes':{k:hashlib.sha256(p.read_bytes()).hexdigest() for k,p in
            [('snapshot',a.snapshot),('profile',a.profile),('source',a.source),('analyze_queue',a.analyze_queue)]},
        'root_projection_sha256':digest,'setup_seconds_including_shared_features':base_seconds,'trials':[]}
    for variant in ('linear','quadratic'):
        current=coords if variant=='linear' else {t:lift(c) for t,c in coords.items()}
        for split in ('new_binding','new_binding_fresh'):
            start=time.perf_counter();predictions={};scope={};models={};held_count=0
            for t,g in groups.items():
                held,scope[t]=train.heldout_split(g,current[t],split);held_count+=int(held.sum())
                scores,prior,models[t]=train.fit(g,current[t],held);predictions[t]=(scores,prior)
            result={'variant':variant,'split':split,'held_out_keys':held_count,
                'fit_predict_seconds':time.perf_counter()-start,
                'weights_and_bias_bytes':sum(m['w'].nbytes+m['bias'].nbytes for m in models.values()),
                'dependent_pairs':train.memory_run.assess(bank,groups,routes,predictions,.5,query_scope=scope),
                'prior':train.memory_run.assess(bank,groups,routes,predictions,.5,True,query_scope=scope),
                'non_constructor_channels':train.condition_subset(groups,predictions,scope,allowed)}
            report['trials'].append(result);print(json.dumps(result),flush=True)
            if variant=='quadratic' and split=='new_binding':
                report['gpu_jobs']=train.write_gpu(a.output/'input.bin',groups,current,models)
                (a.output/'quadratic-audit.json').write_text(json.dumps(diagnose(bank,groups,routes,current,predictions,scope),indent=2)+'\n')
    report['limits']=['Squared coordinates add co-occurrences of observed tokens, not unobserved binding connections.',
        'Equal feature vectors remain indistinguishable; no nonlinear function can recover erased information.',
        'Both splits use the same constructor graph, rule bank and label-producing execution.',
        'GPU numeric equality verifies the coordinate program, not logical correctness of predicted matches.']
    (a.output/'results.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__':main()
