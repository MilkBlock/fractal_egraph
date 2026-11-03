"""Fit once from explicitly exported GPU continuation features and labels.

This is offline model compilation, not an online solver or a new matcher.
No opaque binding handles leave the GPU in this dataset. Production execution
does not export the dataset and evaluates both predicted-positive and negative
continuations exactly on the GPU.
"""
import argparse
import json
import struct
from pathlib import Path
import numpy as np

def train(directory, output, regularization=1.0):
    if regularization <= 0:
        raise ValueError('ridge regularization must be positive')
    layout = json.loads((directory/'context-layout.json').read_text())
    width = layout['ridge']['feature_width']
    heads = layout['ridge']['heads']
    gram = np.zeros((heads,width,width),dtype=np.float64)
    rhs = np.zeros((heads,width),dtype=np.float64)
    count = np.zeros(heads,dtype=np.int64)
    positive = np.zeros(heads,dtype=np.int64)
    with (directory/'context-training.bin').open('rb') as f:
        if f.read(4)!=b'CTX1':
            raise ValueError('context dataset schema')
        while header:=f.read(16):
            if len(header)!=16:raise ValueError('truncated context header')
            rows,w,epoch,overflow=struct.unpack('<4I',header)
            if w!=width or (overflow and rows):raise ValueError('invalid context frame')
            raw=f.read(rows*(w+2)*4)
            if len(raw)!=rows*(w+2)*4:raise ValueError('truncated context data')
            data=np.frombuffer(raw,dtype='<f4').reshape(rows,w+2)
            for head in np.unique(data[:,0]).astype(int):
                selected=data[data[:,0]==head]
                if not 0<=head<heads:raise ValueError('unknown continuation head')
                x=selected[:,1:-1].astype(np.float64);y=selected[:,-1].astype(np.float64)
                if not np.all(np.isfinite(x)) or not np.all((y==0)|(y==1)):
                    raise ValueError('invalid training coordinates or exact label')
                gram[head]+=x.T@x;rhs[head]+=x.T@y
                count[head]+=len(x);positive[head]+=int(y.sum())
    weights=[np.linalg.solve(g+np.eye(width)*regularization,b).tolist() for g,b in zip(gram,rhs)]
    model={'schema':'egglog-context-ridge/v1','layout_sha256':layout['ridge']['layout_sha256'],
           'coefficients':weights,'regularization':regularization,
           'training_contexts':count.tolist(),'positive_contexts':positive.tolist(),
           'scope':'same-run calibration; no held-out accuracy or speedup claim'}
    output.write_text(json.dumps(model,indent=2)+'\n')
    return model

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--run',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--regularization',type=float,default=1.0)
    a=p.parse_args();m=train(a.run,a.output,a.regularization)
    print('trained',sum(m['training_contexts']),'contexts in',len(m['coefficients']),'heads')
