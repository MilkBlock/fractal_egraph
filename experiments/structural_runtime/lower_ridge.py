"""Algebraically lower the existing ridge model; no fitting or new labels.

Old: sum(code(token))/d(count) @ W / 256 + bias.
New: Z @ [high(A), low(A), mask], normalize per output, add the same bias.
A[token,model] = allowed(token,model) * code(token) @ W[:,model] / 256.
"""
import argparse, copy, hashlib, json, sys
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'superposed_key_memory'))
from memory import codes

def lower(lib):
    result=copy.deepcopy(lib);models=lib['models'];v=len(lib['vocabulary'])
    sorts=sorted({m['sort'] for m in models});sort_ids={s:i for i,s in enumerate(sorts)}
    hashes=[t['hash'] for t in lib['vocabulary']]
    for sort in sorts:
        values={m['sort_hash'] for m in models if m['sort']==sort}
        if len(values)!=1:raise ValueError('inconsistent sort encoding')
        hashes.append(values.pop())
    if len(hashes)>2048:raise ValueError('FP16 input-code exactness contract requires <=2048 tokens')
    c=codes(np.array(hashes,dtype=np.uint64),256).astype(np.float64)
    w=np.array([m['weights'] for m in models],dtype=np.uint16).view(np.float16).astype(np.float64).T
    if w.shape!=(256,len(models)) or not np.all(np.isfinite(w)):raise ValueError('invalid original weights')
    mask=np.zeros((len(hashes),len(models)),dtype=np.float64)
    for m,model in enumerate(models):
        excluded=set(model.get('excluded_tokens',[]))
        if any(i<0 or i>=v for i in excluded):raise ValueError('invalid feature mask')
        mask[:v,m]=[i not in excluded for i in range(v)]
        mask[v+sort_ids[model['sort']],m]=1
    exact=(c@w/256)*mask
    high=exact.astype(np.float16);low=(exact-high.astype(np.float64)).astype(np.float16)
    if not np.all(np.isfinite(high)) or not np.all(np.isfinite(low)):raise ValueError('coefficient overflow')
    error=np.abs(exact-high.astype(np.float64)-low.astype(np.float64))
    bounds=[]
    for m in range(len(models)):
        e=np.sort(error[:,m])[::-1];bound=0.0
        for n,total in enumerate(np.cumsum(e),1):
            d=1
            while d*d<n:d*=2
            bound=max(bound,float(total/d))
        bounds.append(bound)
    packed=np.stack([high,low,mask.astype(np.float16)],axis=2).reshape(len(hashes),-1)
    result['resident_matrix']={
        'schema':'resident-ridge-response/v1','token_count':len(hashes),'vocabulary_count':v,
        'sorts':sorts,'model_sorts':[sort_ids[m['sort']] for m in models],
        'weights':packed.view(np.uint16).reshape(-1).tolist(),
        'constant_models':[not any(int(x)&0x7fff for x in m['weights']) for m in models],
        'coefficient_error_bounds':bounds,
        'contract':'Same ridge weights/bias; factorized token response with model masks. High/low FP16 coefficients. Prediction only; exact CPU completion remains mandatory.',
        'rounding_note':'Bounds cover coefficient splitting, not GEMM accumulation order; numerical checks report total score error and threshold changes.'}
    return result

def reference(lib,sort,tokens):
    """Unchanged original FP16-coordinate / FP32-dot reference."""
    output=[]
    for m in lib['models']:
        if m['sort']!=sort:output.append(None);continue
        ts=[m['sort_hash']]+[lib['vocabulary'][i]['hash'] for i in tokens if i not in m.get('excluded_tokens',[])]
        d=1
        while d*d<len(ts):d*=2
        x=(codes(np.array(ts,dtype=np.uint64),256).sum(0)/d).astype(np.float16).astype(np.float32)
        w=np.array(m['weights'],dtype=np.uint16).view(np.float16).astype(np.float32)
        output.append(float(x@w/256+np.float32(m['bias'])))
    return output

def main():
    p=argparse.ArgumentParser();p.add_argument('--model',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();data=a.model.read_bytes();out=lower(json.loads(data))
    out['resident_matrix']['original_model_sha256']=hashlib.sha256(data).hexdigest()
    a.output.write_text(json.dumps(out,separators=(',',':'))+'\n')
    print(json.dumps({'models':len(out['models']),'tokens':out['resident_matrix']['token_count'],'max_coefficient_error_bound':max(out['resident_matrix']['coefficient_error_bounds']),'bytes':a.output.stat().st_size}))
if __name__=='__main__':main()
