"""Check persistent binary GPU service against the unchanged FP16 reference."""
import argparse,json,struct,subprocess,sys
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'superposed_key_memory'))
from memory import codes
def main():
    p=argparse.ArgumentParser()
    for x in ('worker','kernel','model'):p.add_argument('--'+x,type=Path,required=True)
    a=p.parse_args();model=json.loads(a.model.read_text())
    proc=subprocess.Popen([str(a.worker.resolve()),str(a.kernel.resolve()),str(a.model.resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
    def read(n):
        b=b''
        while len(b)<n:
            x=proc.stdout.read(n-len(b))
            if not x:raise RuntimeError('worker disconnected')
            b+=x
        return b
    try:
        assert read(4)==b'RDY1'
        worst=0;count=0
        for size in (1,17,93):
            requests=[];expected=[]
            for m,item in enumerate(model['models']):
                w=np.array(item['weights'],dtype=np.uint16).view(np.float16).astype(np.float32)
                for i in range(size):
                    ts=[item['sort_hash']]+[v['hash'] for j,v in enumerate(model['vocabulary']) if (i+j)%7<2]
                    d=1
                    while d*d<len(ts):d*=2
                    phi=(codes(np.array(ts,dtype=np.uint64),256).sum(0)/d).astype(np.float16).astype(np.float32)
                    requests.append(struct.pack('<II',m,len(ts))+np.array(ts,dtype='<u8').tobytes())
                    expected.append(float(phi@w/256+item['bias']))
            payload=struct.pack('<I',len(requests))+b''.join(requests)
            proc.stdin.write(struct.pack('<I',len(payload))+payload);proc.stdin.flush()
            n,seconds=struct.unpack('<Id',read(12));assert n==len(expected)
            scores=np.frombuffer(read(4*n),dtype='<f4');err=np.max(np.abs(scores-expected));worst=max(worst,float(err))
            assert err<2e-5,(size,err)
            assert np.array_equal(scores>=.5,np.array(expected)>=.5)
            count+=n
        proc.stdin.write(struct.pack('<I',0));proc.stdin.flush();assert proc.wait()==0
        print(json.dumps({'evaluated_scores':count,'max_abs_error':worst,'threshold_disagreements':0,'requests':3}))
    finally:
        if proc.poll() is None:proc.kill();proc.wait()
if __name__=='__main__':main()
