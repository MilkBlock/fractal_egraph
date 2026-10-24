"""Generic finite-set lookup as dense polynomial evaluation over F_65521.

P1/P2 are conservative membership filters. F1 returns a first-hash bucket,
whose original keys/row payloads are verified exactly by the caller. No Cell
recurrence, scalar affine law, or sampled matching labels are used.
"""
import numpy as np

PRIME=65521


def evaluate(coeff,x):
    x=np.asarray(x,dtype=np.int64);result=np.zeros_like(x)
    for c in reversed(coeff):result=(result*x+int(c))%PRIME
    return result


def insert_root(coeff,x):
    return (np.r_[0,coeff]-int(x)*np.r_[coeff,0])%PRIME


def remove_root(coeff,x):
    if int(evaluate(coeff,x))!=0:raise ValueError('cannot remove a non-root')
    out=np.zeros(len(coeff)-1,dtype=np.int64);out[-1]=coeff[-1]
    for i in range(len(out)-2,-1,-1):out[i]=(coeff[i+1]+int(x)*out[i+1])%PRIME
    return out


def interpolate(roots,values,p):
    roots=np.asarray(roots,dtype=np.int64);values=np.asarray(values,dtype=np.int64)
    if not len(roots):return np.zeros(1,dtype=np.int64)
    derivative=(np.arange(1,len(p))*p[1:])%PRIME
    slopes=evaluate(derivative,roots)
    if np.any(slopes==0):raise ValueError('interpolation roots must be distinct')
    weights=np.array([int(v)*pow(int(d),-1,PRIME)%PRIME for v,d in zip(values,slopes)],dtype=np.int64)
    q=np.ones(len(roots),dtype=np.int64);out=np.zeros(len(roots),dtype=np.int64)
    for i in range(len(roots)-1,-1,-1):
        out[i]=int(weights@q)%PRIME
        q=(p[i]+roots*q)%PRIME
    return out


class PolynomialIndex:
    def __init__(self,roots):
        # Hash buckets are stable, explicit payload handles. Colliding original
        # keys must all be retained in the same bucket, never choose one witness.
        self.roots=list(sorted(set(map(int,roots))))
        self.handles={root:i for i,root in enumerate(self.roots)}
        self.p=np.array([1],dtype=np.int64)
        for root in self.roots:self.p=insert_root(self.p,root)
        self.f=interpolate(self.roots,list(range(len(self.roots))),self.p)

    def insert(self,root,handle):
        root=int(root)
        if root in self.handles:raise ValueError('existing bucket payload update requires no polynomial insertion')
        if not 0<=handle<PRIME:raise ValueError('payload handle outside field')
        # Fnew=F+(handle-F(root))/P(root)*P; Pnew=(z-root)P.
        alpha=(int(handle)-int(evaluate(self.f,root)))*pow(int(evaluate(self.p,root)),-1,PRIME)%PRIME
        self.f=(np.pad(self.f,(0,len(self.p)-len(self.f)))+alpha*self.p)%PRIME
        self.p=insert_root(self.p,root);self.handles[root]=handle
        self.roots=sorted(self.handles)

    def remove(self,root):
        root=int(root)
        if root not in self.handles:raise ValueError('no such bucket')
        pnew=remove_root(self.p,root)
        # F modulo the new membership polynomial preserves all other values.
        f=np.pad(self.f,(0,max(0,len(pnew)-len(self.f))))
        if len(f)==len(pnew):f=(f-int(f[-1])*pnew)%PRIME;f=f[:-1]
        self.f=f if len(f) else np.zeros(1,dtype=np.int64)
        self.p=pnew;del self.handles[root]
        self.roots=sorted(self.handles)


def hash_keys(keys,seed=441):
    # Raw native canonical IDs / i64 bit patterns, NOT target-table dictionary
    # ordinals: a per-column dictionary could already solve a unary lookup.
    keys=np.asarray(keys,dtype='<u8')
    raw=keys.view(np.uint8).reshape(len(keys),-1).astype(np.int64)
    rng=np.random.default_rng(seed)
    weights=rng.integers(0,PRIME,size=(raw.shape[1],2),dtype=np.int64)
    bias=rng.integers(0,PRIME,size=2,dtype=np.int64)
    return (raw@weights+bias)%PRIME


def digits(values):
    values=np.asarray(values,dtype=np.int64)%PRIME
    signed=np.where(values>PRIME//2,values-PRIME,values)
    lo=(signed+128)%256-128;hi=(signed-lo)//256
    assert np.max(np.abs(lo),initial=0)<=128 and np.max(np.abs(hi),initial=0)<=128
    return lo.astype(np.float16),hi.astype(np.float16)


def gemm_mod(a,b):
    """Exact FP16 inputs/FP32 accumulated dot products up to width 512.

    65536=15 mod PRIME, so reconstruction uses small modular coefficients.
    """
    if a.shape[1]>512:raise ValueError('split into <=512-wide contractions')
    al,ah=digits(a);bl,bh=digits(b)
    parts=[x.astype(np.float32)@y.astype(np.float32) for x,y in ((al,bl),(al,bh),(ah,bl),(ah,bh))]
    parts=[np.rint(x).astype(np.int64)%PRIME for x in parts]
    return (parts[0]+256*(parts[1]+parts[2])+15*parts[3])%PRIME
