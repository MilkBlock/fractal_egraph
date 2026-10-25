"""Resident ridge/Cell correctness runner. No Pass-3 acceleration claim."""
import argparse,copy,hashlib,json,struct,subprocess,sys
from pathlib import Path
import numpy as np
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from experiments.resident_ridge.compiler import compile_library,fit,active_features,guard,expand_graph,train
from tools.bench.math_graph_equal import verify
HERE=Path(__file__).resolve().parent

class Resident:
    def __init__(self,worker,program,mode):
        self.p=subprocess.Popen([str(worker.resolve()),str(program.resolve()),str(HERE/'kernel.metal'),mode],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        self.upload=0;self.download=0;self.commands=[]
        if self.read(4)!=b'RDR1':raise RuntimeError('wrong worker')
    def read(self,n):
        data=b''
        while len(data)<n:
            x=self.p.stdout.read(n-len(data))
            if not x:raise RuntimeError(self.p.stderr.read().decode())
            data+=x
        return data
    def send(self,words):
        data=np.array(words,dtype='<u4').tobytes();self.p.stdin.write(struct.pack('<I',len(data))+data);self.p.stdin.flush()
        n,seconds,resident=struct.unpack('<IdQ',self.read(20));payload=self.read(n*4)
        self.upload+=len(data)+4;self.download+=20+n*4
        self.commands.append(dict(opcode=words[0],uploaded=len(data)+4,downloaded=20+n*4,gpu_seconds=seconds,resident_bytes=resident))
        return payload
    def close(self):
        if self.p.poll() is None:
            self.p.stdin.write(struct.pack('<I',0));self.p.stdin.flush()
            if self.p.wait()!=0:raise RuntimeError(self.p.stderr.read().decode())
        for pipe in (self.p.stdin,self.p.stdout,self.p.stderr):pipe.close()


def fixture():
    cases=[dict(parameters=[1000+11*i+j for j in range(4)],ready=15) for i in range(32)]
    for i in range(16,24):cases[i]['ready']=3
    for i,target,source in [(24,1,0),(25,2,0),(26,3,1),(27,2,1)]:cases[i]['parameters'][target]=cases[i]['parameters'][source]
    cases[28]['ready']=0;cases[29]['ready']=3;cases[30]['ready']=7;cases[31]['ready']=12
    patches=[(i,12) for i in range(16,24)]+[(31,3)]
    return cases,patches


def reference(p,cases,patches):
    results=[]
    for row,c in enumerate(cases):
        state=0;status=0;frame=[0]*p['slots'];events=[];ready=c['ready'];params=c['parameters']
        for phase in range(3):
            if phase==1:ready|=dict(patches).get(row,0)
            for _ in range(p['sweeps']):
                if status:continue
                enabled=[i for i,t in enumerate(p['transitions']) if guard(p,t,state,ready,params,frame)==1]
                if not enabled:
                    if any(guard(p,t,state,ready,params,frame)==2 for t in p['transitions']):status=2
                    elif not any(t['source']==state for t in p['transitions']):status=1
                    continue
                assert len(enabled)==1
                tid=enabled[0];t=p['transitions'][tid];inputs=frame[:p['sizes'][state]]+[params[i] for i in t['parameters']]
                frame=[inputs[i] if i>=0 else p['fresh_base']+row*p['fresh_stride']+p['state_offsets'][t['target']]+s for s,i in enumerate(t['mapping'])]
                frame += [0]*(p['slots']-len(frame));state=t['target'];events.append(tid)
                if not any(t['source']==state for t in p['transitions']):status=1
        results.append(dict(state=state,status=status,ready=ready,parameters=params,frame=frame,events=events))
    return results


def decode(p,data):
    words=np.frombuffer(data,dtype='<u4').reshape(-1,4+len(p['parameter_names'])+p['slots']+len(p['transitions'])+8);result=[]
    for row in words.tolist():
        state,status,ready,n=row[:4];at=4;params=row[at:at+len(p['parameter_names'])];at+=len(params)
        frame=row[at:at+p['slots']];at+=p['slots'];events=row[at:at+len(p['transitions'])][:n];at+=len(p['transitions'])
        result.append(dict(state=state,status=status,ready=ready,parameters=params,frame=frame,events=events,stats=row[at:]))
    return result


def native_shapes(p,lib,records,oracle,out):
    out.mkdir(parents=True,exist_ok=True)
    (out/'cases.json').write_text(json.dumps(dict(parameter_names=p['parameter_names'],cases=records)))
    subprocess.run([str(oracle),'verify',str(HERE/'math.egg'),str(HERE/'plan.json'),str(out/'cases.json'),str(out/'native')],check=True,stdout=subprocess.DEVNULL)
    native=json.loads((out/'native/report.json').read_text());graphs=[]
    for i,record in enumerate(records):
        actual=expand_graph(p,lib,record);(out/f'{i}-gpu.json').write_text(json.dumps(actual))
        snap=json.loads((out/f'native/{i}.json').read_text())
        if snap is None:
            assert record['state']==0 and not record['events'];continue
        values=snap['values'];expected=[]
        for r in snap['rows']:
            args=[[values[a]['sort'],values[a]['literal'] if values[a]['literal'] is not None else str(a)] for a in r['args']]
            expected.append(dict(op=r['op'],args=args,result=str(r['result'])))
        (out/f'{i}-native.json').write_text(json.dumps(expected));graphs.append(verify(out/f'{i}-gpu.json',out/f'{i}-native.json'))
    return dict(native_transitions=sum(c['verified_transitions'] for c in native['cases']),expanded_graphs=len(graphs),
        native_smooth_matches=sum(sum(x['native_smooth_matches'].values()) for c in native['cases'] for x in c['comparisons']),
        commit_smooth_matches=sum(sum(c['original_smooth_queries'].values()) for c in native['cases']),
        full_graph_checks=graphs,passed=True)


def run_variant(p,lib,worker,oracle,out,mode):
    out.mkdir(parents=True,exist_ok=False);path=out/'program.json';path.write_text(json.dumps(p))
    cases,patches=fixture();ref=reference(p,cases,patches);r=Resident(worker,path,mode)
    try:
        init=[1,len(cases)]+[v for c in cases for v in c['parameters']+[c['ready']]]
        r.send(init);r.send([2,p['sweeps']])
        # External readiness is given independently of GPU answers. No state,
        # scores or partial bindings are read between these commands.
        r.send([3,len(patches)]+[v for patch in patches for v in patch]);r.send([2,p['sweeps']]);r.send([2,p['sweeps']])
        actual=decode(p,r.send([4]));production_commands=copy.deepcopy(r.commands)
        for a,b in zip(actual,ref):
            assert {k:a[k] for k in b}==b,(a,b)
            for slot,param in p['consumed'][a['state']]:assert a['frame'][slot]==a['parameters'][param], 'named interface port changed'
        # Final checkpoint numerical audit: recompute scores without advancing.
        raw=np.frombuffer(r.send([5]),dtype='<f4').reshape(len(cases),256+len(p['transitions']))
        code=np.array(p['codes'],dtype=np.int32);weight=np.array(p['ridge']['weights'],dtype=np.uint16).view(np.float16).astype(np.float32);bias=np.array(p['ridge']['bias'],dtype=np.float32)
        if mode!='learned':weight[:]=0;bias[:]=-1 if mode=='negative' else 1
        expected=[]
        for a in actual:
            f=active_features(p,a['state'],a['ready'],a['parameters']);phi=(np.array(f)@code/train.divisor(sum(f))).astype(np.float16).astype(np.float32);expected.append(phi)
        expected=np.array(expected);assert np.array_equal(expected,raw[:,:256]),'incremental coordinates differ from full recomputation'
        scores=expected@weight/256+bias;error=float(np.max(np.abs(scores-raw[:,256:])))
        assert error<2e-5,error
        result=dict(mode=mode,basis=p.get('coordinate_basis','original'),contexts=len(cases),completed=sum(a['status']==1 for a in actual),waiting=sum(a['status']==0 for a in actual),
            needs_specialization=sum(a['status']==2 for a in actual),transitions=sum(len(a['events']) for a in actual),
            rejected_proposals=sum(a['stats'][2] for a in actual),recovered_transitions=sum(a['stats'][3] for a in actual),
            state_transfers_during_gpu_steps=0,production_commands=production_commands,numeric_error=error,
            exact_incremental_coordinates=True,numeric_audit_transfer=r.commands[-1],records=actual)
    finally:r.close()
    (out/'gpu.json').write_text(json.dumps(result,indent=2)+'\n')
    result['native']=native_shapes(p,lib,actual,oracle,out/'check')
    (out/'report.json').write_text(json.dumps(result,indent=2)+'\n');return result


def main():
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    info=json.loads((a.build/'build.json').read_text());oracle=Path(info['binary'])
    subprocess.run([str(oracle),'bake',str(HERE/'math.egg'),str(HERE/'plan.json'),str(a.output/'library.json')],check=True)
    lib=json.loads((a.output/'library.json').read_text());program=fit(compile_library(lib))
    (a.output/'program.json').write_text(json.dumps(program))
    results=[]
    for mode in ['learned','negative','positive']:
        results.append(run_variant(program,lib,a.build/'worker',oracle,a.output/mode,mode))
    rotated=copy.deepcopy(program);rng=np.random.default_rng(7);perm=rng.permutation(256);sign=rng.choice([-1,1],256)
    rotated['coordinate_basis']='signed-permutation'
    rotated['basis_permutation']=perm.tolist();rotated['basis_signs']=sign.tolist()
    rotated['codes']=(np.array(program['codes'])[:,perm]*sign).tolist()
    w=np.array(program['ridge']['weights'],dtype=np.uint16).view(np.float16)
    rotated['ridge']['weights']=(w[perm]*sign[:,None]).astype(np.float16).view(np.uint16).tolist()
    results.append(run_variant(rotated,lib,a.build/'worker',oracle,a.output/'orthogonal','learned'))
    compact=[]
    for r in results:compact.append({k:v for k,v in r.items() if k!='records'})
    report=dict(scope='Standalone resident ridge/whole-Cell prototype, using native Bake and real native constructor/union commit. Deterministic acyclic Math transfer library; not general eggcc or production GPU union/hashcons.',
        training=program['training'],results=compact,build=info,source_hashes={str(x):hashlib.sha256(x.read_bytes()).hexdigest() for x in [HERE/'run.py',HERE/'build.py',HERE/'compiler.py',HERE/'worker.swift',HERE/'kernel.metal',HERE/'math.egg',HERE/'plan.json']})
    (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ['scope','training']},indent=2))
    print(json.dumps([{k:r[k] for k in ['mode','completed','waiting','needs_specialization','transitions','rejected_proposals','recovered_transitions','numeric_error']} for r in results],indent=2))
if __name__=='__main__':main()
