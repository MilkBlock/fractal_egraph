"""Native Pass-3 stage sampling and chronological Polyspine ridge ablations.

Offline representation experiment. CPU extraction/fit and GPU score evaluation
are accounted separately; there is no runtime acceleration claim.
"""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import time
import numpy as np
from features import STAGES, View, labels, stage_spans

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / 'matrix_closure'))
from verify_dynamic import Oracle
sys.path.insert(0, str(HERE.parent / 'superposed_key_memory'))
from memory import codes, fingerprint

VARIANTS = ['structure', 'stage', 'interfaces', 'polyspine', 'shuffled_seams']
sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()


def collect(workload, frontend, out):
    if out.exists():
        raise ValueError('capture output already exists; reuse it with fit')
    out.mkdir(parents=True)
    saved = json.loads(workload.read_text())
    source = Path(saved['source'])
    if sha(source) != saved['source_sha256'] or sha(frontend) != saved['native_frontend_sha256']:
        raise ValueError('native source/binary differs from verified schedule')
    spans, all_spans = stage_spans(saved['core']['schedule'], saved['native_trace'])
    native = Oracle(frontend, source, out)
    vocabulary = []; token_ids = {}; signatures = []; signature_ids = {}
    def intern(first, second):
        def bag(tokens):
            ids = []
            for token in tokens:
                if token not in token_ids:
                    token_ids[token] = len(vocabulary); vocabulary.append(token)
                ids.append(token_ids[token])
            return tuple(ids)
        key = (bag(first), bag(second))
        if key not in signature_ids:
            signature_ids[key] = len(signatures); signatures.append(key)
        return signature_ids[key]
    view = View(native.core)
    at = 0; previous = 'initial'; records = []; native_seconds = 0.0
    def advance(end):
        nonlocal at, native_seconds
        if end <= at:
            return
        expected = saved['native_trace'][at:end]
        r = native.ask({'op': 'time_runs', 'sets': [x['set'] for x in expected]})
        if [x['updated'] for x in r['steps']] != [x['updated'] for x in expected]:
            raise AssertionError(f'native update trace mismatch from {at} to {end}')
        native_seconds += r['seconds']; at = end
    try:
        for i, span in enumerate(spans):
            advance(span['start'])
            start = time.perf_counter(); before = native.ask({'op': 'snapshot'})
            snapshot_seconds = time.perf_counter() - start
            start = time.perf_counter(); rows, stats = view.extract(before, span['stage'], previous)
            extraction_seconds = time.perf_counter() - start
            # Freeze every feature before execution or observation of its label.
            start = time.perf_counter(); ids = np.zeros((len(rows), len(VARIANTS)), np.uint32)
            shuffle = np.random.default_rng(1729 + i).permutation(len(rows))
            for j, (_, base, stage, local, poly, _) in enumerate(rows):
                seam = set(rows[int(shuffle[j])][4]) - set(rows[int(shuffle[j])][3])
                for v, second in enumerate([(), stage, local, poly, tuple(sorted(set(local) | seam, key=repr))]):
                    ids[j, v] = intern(base, second)
            packing_seconds = time.perf_counter() - start
            native.ask({'op': 'track_anchors'})
            advance(span['end'])
            start = time.perf_counter(); post = native.ask({'op': 'snapshot'})
            snapshot_seconds += time.perf_counter() - start
            keys = [r[0] for r in rows]
            y = np.array(labels(post, keys, STAGES[span['stage']][1]), np.uint8)
            present = np.array([r[5] for r in rows], np.uint8)
            name = f'frame-{i:03}.npz'
            np.savez_compressed(out / name, signatures=ids, y=y, before=present)
            record = dict(span, file=name, rows=len(rows), positives=int(y.sum()),
                          absent_before=int((present == 0).sum()),
                          new_positive=int(((present == 0) & (y == 1)).sum()),
                          snapshot_seconds=snapshot_seconds, extraction_seconds=extraction_seconds,
                          packing_seconds=packing_seconds, **{'features': stats})
            records.append(record); previous = span['stage']
            print(json.dumps({k: record[k] for k in ['stage','cycle','rows','positives','new_positive','extraction_seconds']}), flush=True)
            (out / 'progress.json').write_text(json.dumps({'frames': len(records), 'native_runs': at}))
            del before, post, rows
        advance(len(saved['native_trace']))
        data = {'schema': 'polyspine-ridge/v1', 'scope': 'offline native Pass-3 stage-boundary prediction',
                'source': str(source), 'source_sha256': sha(source), 'frontend_sha256': sha(frontend),
                'workload': str(workload.resolve()), 'workload_sha256': sha(workload),
                'capture_sources': {p.name: sha(p) for p in [HERE/'study.py', HERE/'features.py']},
                'native_runs': at, 'native_trace_exact': True, 'native_seconds_diagnostic': native_seconds,
                'eligible_stage_invocations': all_spans, 'frames': records,
                'selection': 'first Saturate of each declared family per outer optimization cycle; all other Runs executed',
                'variants': VARIANTS, 'vocabulary': vocabulary, 'signatures': signatures,
                'feature_boundary': 'all feature bags frozen before native stage execution; active-stage analysis outputs masked',
                'target': 'existence of target-table row at pre-existing Expr key after original Saturate; values/bindings are not predicted',
                'limits': ['No claim of automatic whole-cell recognition, full matching or runtime speedup.',
                           'Writer tags are static possible producers; no per-row causal attribution.',
                           'CPU snapshot/feature extraction is offline and explicitly charged.',
                           'No new Expr key born during target stage is in this fixed pre-stage prediction domain.']}
        (out/'dataset.json').write_text(json.dumps(data, separators=(',', ':')))
    except Exception as error:
        (out/'failure.json').write_text(json.dumps({'error': str(error), 'native_runs': at, 'frames': records}, indent=2))
        raise
    finally:
        native.close()


def counts(y, score, weights=None):
    guess = score >= .5; y = y.astype(bool)
    w = np.ones(len(y), dtype=np.int64) if weights is None else weights
    tp = int(w[y & guess].sum()); fp = int(w[~y & guess].sum())
    fn = int(w[y & ~guess].sum()); tn = int(w[~y & ~guess].sum())
    return {'tp': tp, 'fp': fp, 'fn': fn, 'tn': tn,
            'precision': tp/(tp+fp) if tp+fp else None,
            'recall': tp/(tp+fn) if tp+fn else None,
            'accuracy': (tp+tn)/(tp+fp+fn+tn) if tp+fp+fn+tn else None,
            'brier': float(np.sum(w*(score-y)**2)/w.sum()) if w.sum() else None}


def fit_model(x, n, pos, regularization=1.0):
    take = n > 0; x = x[take].astype(np.float64); n = n[take]; pos = pos[take]
    if not n.sum():
        raise ValueError('empty training split')
    mx = n @ x / n.sum(); my = pos.sum() / n.sum(); centered = x-mx
    gram = centered.T @ (n[:, None]*centered) + np.eye(x.shape[1])*regularization
    rhs = centered.T @ (pos - n*my)
    theta = np.linalg.solve(gram, rhs)
    # Match the established GPU worker's x @ W / width + bias convention.
    w = (theta*x.shape[1]).astype(np.float16); bias = np.float32(my-mx@theta)
    if not np.all(np.isfinite(w)):
        raise ValueError('FP16 coefficient overflow')
    return w, bias, float(my)


def fit(directory, out, worker=None):
    out.mkdir(parents=True, exist_ok=False)
    data = json.loads((directory/'dataset.json').read_text())
    stages = list(STAGES); cycles = sorted({f['cycle'] for f in data['frames']})
    split = cycles[len(cycles)//2]
    # Fixed width, ridge penalty and threshold across all feature definitions.
    start = time.perf_counter()
    hashes = np.array([fingerprint(t) for t in data['vocabulary']], dtype=np.uint64)
    token_codes = codes(hashes, 256).astype(np.float32)
    x = np.zeros((len(data['signatures']),512), np.float16)
    for i, bags in enumerate(data['signatures']):
        for block, bag in enumerate(bags):
            div = 1
            while div*div < len(bag): div *= 2
            if bag: x[i, block*256:(block+1)*256] = token_codes[bag].sum(0)/div
    encode_seconds = time.perf_counter()-start
    # Summarize identical vectors without discarding conflicting labels.
    ns = len(x); nh = len(stages); nv = len(VARIANTS)
    train_n = np.zeros((nv,nh,ns), np.int64); train_p = train_n.copy()
    test_n = train_n.copy(); test_p = train_n.copy(); pending_n = train_n.copy(); pending_p = train_n.copy()
    for f in data['frames']:
        frame = np.load(directory/f['file']); ids = frame['signatures']; y=frame['y']; pending=frame['before']==0
        h = stages.index(f['stage']); n,p = (train_n,train_p) if f['cycle']<split else (test_n,test_p)
        for v in range(nv):
            np.add.at(n[v,h],ids[:,v],1);np.add.at(p[v,h],ids[:,v],y)
            if f['cycle']>=split:
                np.add.at(pending_n[v,h],ids[pending,v],1);np.add.at(pending_p[v,h],ids[pending,v],y[pending])
    def metric(n,p,score):
        take=n>0;n=n[take];p=p[take];score=score[take]
        # Evaluate exact weighted positive and negative populations separately.
        return counts(np.r_[np.ones(len(n)),np.zeros(len(n))],np.r_[score,score],np.r_[p,n-p])
    records=[];gpu_jobs=[];refs=[];model_weights={};predictions={}
    for v, variant in enumerate(VARIANTS):
        started=time.perf_counter();heads=[]
        for h, stage in enumerate(stages):
            w,b,prior=fit_model(x,train_n[v,h],train_p[v,h]);score=x.astype(np.float32)@w.astype(np.float32)/512+b
            model_weights[(v,h)]=(w,b);predictions[(v,h)]=score
            n=test_n[v,h];p=test_p[v,h]
            ambiguous=(p>0)&(p<n)
            row={'stage':stage,'target':STAGES[stage][1], 'train_rows':int(train_n[v,h].sum()),
                 'test_rows':int(n.sum()),'test':metric(n,p,score),
                 'pending':metric(pending_n[v,h],pending_p[v,h],score),
                 'prior':metric(n,p,np.full(ns,prior)),
                 'unseen_signature_test':metric(n*(train_n[v,h]==0),p*(train_n[v,h]==0),score),
                 'contradictory_test_signatures':int(ambiguous.sum()),
                 'signature_irreducible_errors':int(np.minimum(p,n-p).sum())}
            heads.append(row)
        records.append({'variant':variant,'heads':heads,'fit_and_score_seconds':time.perf_counter()-started})
        print(variant,[(r['stage'],r['test']['precision'],r['test']['recall'],r['pending']['recall']) for r in heads],flush=True)
    # Use the same established Apple simdgroup GEMM, GPU token encoding and
    # FP16 weights. No replacement inference engine or CPU-computed bindings.
    with (out/'input.bin').open('wb') as stream:
        stream.write(b'SPINE003');stream.write(struct.pack('<I',nv))
        for v,variant in enumerate(VARIANTS):
            used=np.flatnonzero(test_n[v].sum(0)>0);w=np.column_stack([model_weights[(v,h)][0] for h in range(nh)])
            b=np.array([model_weights[(v,h)][1] for h in range(nh)],np.float32)
            stream.write(struct.pack('<3I',len(used),512,nh))
            for i in used:
                for bag in data['signatures'][i]:
                    stream.write(struct.pack('<I',len(bag)));stream.write(hashes[bag].astype('<u8').tobytes())
            stream.write(w.astype('<f2').tobytes());stream.write(b.astype('<f4').tobytes())
            refs.append((x[used].astype(np.float32)@w.astype(np.float32)/512+b).ravel())
            gpu_jobs.append({'variant':variant,'matrix':[len(used),512,nh],
                             'relevant_scores':int((test_n[v]>0).sum()),'emitted_scores':len(used)*nh})
    np.concatenate(refs).astype('<f4').tofile(out/'reference.bin')
    gpu=None
    if worker:
        with (out/'gpu.log').open('w') as log:
            subprocess.run([str(worker.resolve()),str(out/'input.bin'),str(out/'reference.bin'),str(out/'gpu.json')],check=True,stdout=log,stderr=log)
        gpu=json.loads((out/'gpu.json').read_text())
    paired = []
    for first, second in [('structure','polyspine'),('interfaces','polyspine'),('shuffled_seams','polyspine')]:
        a,b=VARIANTS.index(first),VARIANTS.index(second)
        for h,stage in enumerate(stages):
            totals=defaultdict(int);examples=[]
            for f in data['frames']:
                if f['cycle']<split or f['stage']!=stage:continue
                z=np.load(directory/f['file']);y=z['y'].astype(bool);ids=z['signatures']
                pa=predictions[(a,h)][ids[:,a]]>=.5;pb=predictions[(b,h)][ids[:,b]]>=.5
                fixed=(pa!=y)&(pb==y);broken=(pa==y)&(pb!=y)
                totals['fixed']+=int(fixed.sum());totals['broken']+=int(broken.sum())
                pending=z['before']==0
                totals['pending_fixed']+=int((fixed&pending).sum());totals['pending_broken']+=int((broken&pending).sum())
                for j in np.flatnonzero(fixed|broken):
                    if len(examples)>=4:break
                    sig=int(ids[j,b]);examples.append({'frame':f['file'],'row':int(j),'truth':bool(y[j]),
                        'before_score':float(predictions[(a,h)][ids[j,a]]),'after_score':float(predictions[(b,h)][sig]),
                        'features':[[data['vocabulary'][t] for t in bag] for bag in data['signatures'][sig]]})
            paired.append({'before':first,'after':second,'stage':stage,**dict(totals),'examples':examples})
    report={'schema':'polyspine-ridge-results/v1','scope':data['scope'],
            'dataset':str(directory.resolve()),'dataset_sha256':sha(directory/'dataset.json'),
            'source_sha256':data['source_sha256'],'frontend_sha256':data['frontend_sha256'],
            'study_sources':{p.name:sha(p) for p in [HERE/'study.py',HERE/'features.py']},
            'native_runs':data['native_runs'],'native_trace_exact':data['native_trace_exact'],
            'frames':len(data['frames']),'stages':stages,'variants':records,
            'split':{'kind':'chronological outer-cycle holdout','train_cycles':[c for c in cycles if c<split],
                     'test_cycles':[c for c in cycles if c>=split],
                     'same_binding_can_recur':'yes; future stages, not unseen-program generalization'},
            'width':512,'regularization':1.0,'threshold':.5,'encoded_signatures':ns,
            'offline_encoding_seconds':encode_seconds,
            'offline_extraction_seconds':sum(f['extraction_seconds'] for f in data['frames']),
            'offline_packing_seconds':sum(f['packing_seconds'] for f in data['frames']),
            'diagnostic_snapshot_seconds':sum(f['snapshot_seconds'] for f in data['frames']),
            'jobs':gpu_jobs,'gpu':gpu,'worker_sha256':sha(worker) if worker else None,'paired':paired,
            'limits':data['limits']+['Scores predict projected answer existence, not full rule matches or RHS effects.',
                                  'All predicted negatives require exact fallback; no matches are skipped by this experiment.',
                                  'Current-stage features use earlier committed facts, never post-stage labels.',
                                  'GPU evaluation includes extra unused head scores; no throughput utilization claim.']}
    (out/'results.json').write_text(json.dumps(report,indent=2)+'\n')
    return report


if __name__=='__main__':
    p=argparse.ArgumentParser();sub=p.add_subparsers(dest='command',required=True)
    c=sub.add_parser('collect');c.add_argument('--workload',required=True,type=Path);c.add_argument('--frontend',required=True,type=Path);c.add_argument('--output',required=True,type=Path)
    f=sub.add_parser('fit');f.add_argument('--dataset',required=True,type=Path);f.add_argument('--output',required=True,type=Path);f.add_argument('--worker',type=Path)
    a=p.parse_args()
    if a.command=='collect':collect(a.workload,a.frontend,a.output)
    else:fit(a.dataset,a.output,a.worker)
