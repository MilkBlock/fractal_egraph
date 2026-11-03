"""Lossy, additive key memory for dense matrix probing.

GPU-side state is a signed coefficient matrix, not an exact key dictionary.
It suggests candidates only. CPU validation AND coverage of rejected/unknown
candidates remain necessary for completeness.
"""
import hashlib
import json
import numpy as np

MASK=(1<<64)-1
GOLD=np.uint64(0x9e3779b97f4a7c15)
SHARD=np.uint64(0xc6bc279692b5cc83)


def mix(x):
    with np.errstate(over='ignore'):
        x=np.asarray(x,dtype=np.uint64)
        x=(x^(x>>np.uint64(30)))*np.uint64(0xbf58476d1ce4e5b9)
        x=(x^(x>>np.uint64(27)))*np.uint64(0x94d049bb133111eb)
        return x^(x>>np.uint64(31))


def fingerprint(key):
    # Hash actual typed canonical IDs/literals, not target-table ordinals.
    return int.from_bytes(hashlib.blake2b(json.dumps(key,separators=(',',':')).encode(),digest_size=8,person=b'cell-key').digest(),'little')


def codes(keys,width):
    if width<=0 or width%64:raise ValueError('width must be a positive multiple of 64')
    keys=np.asarray(keys,dtype=np.uint64);out=np.empty((len(keys),width),dtype=np.float32)
    shifts=np.arange(64,dtype=np.uint64)
    for w in range(width//64):
        salt=np.uint64(((w+1)*int(GOLD))&MASK)
        words=mix(keys^salt)
        out[:,w*64:(w+1)*64]=2*((words[:,None]>>shifts)&np.uint64(1)).astype(np.float32)-1
    return out


def shards(keys,count):
    if count<=0 or count&(count-1):raise ValueError('shards must be a power of two')
    return (mix(np.asarray(keys,dtype=np.uint64)^SHARD)&np.uint64(count-1)).astype(np.int64)


class Memory:
    def __init__(self,keys,payload,width=256,parts=16,center=True,fit=False):
        self.width=width;self.parts=parts;self.channels=payload.shape[1]
        self.prior=np.zeros((parts,self.channels),dtype=np.float32)
        self.weights=np.zeros((parts,width,self.channels),dtype=np.float32)
        partition=shards(keys,parts)
        self.population=np.bincount(partition,minlength=parts)
        self.fit=fit;self.training_max_error=0.
        for s in range(parts):
            take=np.flatnonzero(partition==s)
            if not len(take):continue
            y=payload[take].astype(np.float32)
            if center:self.prior[s]=(y.mean(0)>.5).astype(np.float32)
            r=codes(np.asarray(keys)[take],width)
            if fit:
                if len(take)>=width:raise ValueError('interpolation needs more coordinates than keys; split shards, do not discard keys')
                # Store the same scaled matrix as the additive memory. Query
                # still evaluates prior + code @ W / width on the device.
                gram=r@r.T+np.eye(len(take),dtype=np.float32)*.001
                self.weights[s]=width*(r.T@np.linalg.solve(gram,y-self.prior[s]))
            else:self.weights[s]=r.T@(y-self.prior[s])
        # All values are integer sums. If FP16 cannot exactly store a weight,
        # reject this configuration instead of silently changing the model.
        half=self.weights.astype(np.float16)
        if not fit and not np.array_equal(half.astype(np.float32),self.weights):raise ValueError('coefficient exceeds exact FP16 range')
        if not np.all(np.isfinite(half)):raise ValueError('FP16 coefficient overflow')
        self.weights=half
        if fit:
            for s in range(parts):
                take=np.flatnonzero(partition==s)
                if not len(take):continue
                score=self.prior[s]+codes(np.asarray(keys)[take],width)@self.weights[s].astype(np.float32)/width
                self.training_max_error=max(self.training_max_error,float(np.max(np.abs(score-payload[take]))))
                if not np.array_equal(score>=.5,payload[take].astype(bool)):raise ValueError('known key classification failed after FP16 storage')

    def predict(self,keys):
        keys=np.asarray(keys,dtype=np.uint64);partition=shards(keys,self.parts)
        out=np.empty((len(keys),self.channels),dtype=np.float32)
        prior=np.empty_like(out)
        for s in range(self.parts):
            take=np.flatnonzero(partition==s)
            if not len(take):continue
            prior[take]=self.prior[s]
            out[take]=self.prior[s]+codes(keys[take],self.width)@self.weights[s].astype(np.float32)/self.width
        return out,prior

    def update_existing_key(self,key,delta):
        if self.fit:raise ValueError('fitted memory must be refit or use a maintained inverse; additive update would disturb prior guarantees')
        # Caller supplies a net logical-membership delta, not every physical
        # insert. Last-witness removal / canonicalization remain CPU duties.
        s=int(shards([key],self.parts)[0]);w=self.weights[s].astype(np.float32)
        w+=codes([key],self.width).T@np.asarray(delta,dtype=np.float32)[None,:]
        half=w.astype(np.float16)
        if not np.array_equal(half.astype(np.float32),w):raise ValueError('update exceeds exact FP16 storage')
        self.weights[s]=half


def interpolation_parts(keys,width):
    # Capacity is a dimension constraint, not a key/candidate budget. Every key
    # remains. Leave explicit rank margin, then verify the actual bucket load.
    parts=1
    while len(keys)>parts*(3*width//4):parts*=2
    while np.max(np.bincount(shards(keys,parts),minlength=parts),initial=0)>=width:parts*=2
    return parts


def condition_counts(expected,predicted):
    tp=int(np.count_nonzero(expected&predicted));fp=int(np.count_nonzero(~expected&predicted))
    fn=int(np.count_nonzero(expected&~predicted));tn=int(np.count_nonzero(~expected&~predicted))
    return {'true_positive':tp,'false_positive':fp,'missed_true':fn,'true_negative':tn,
        'precision':tp/(tp+fp) if tp+fp else None,'recall':tp/(tp+fn) if tp+fn else None,
        'proposed':tp+fp,'cpu_complement_space':fn+tn}
