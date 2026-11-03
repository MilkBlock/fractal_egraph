"""Check resident incremental responses against the OLD ridge reference.

Includes growth, multiple heads per context, no-update reuse, sparse changes,
model-specific masks, and stale-version rejection. This is not a timing bench.
"""
import argparse,json,os,struct,subprocess,tempfile
from pathlib import Path
import numpy as np
from lower_ridge import reference,lower

class Session:
    def __init__(self,worker,kernel,model,scope_mode='consumers'):
        self.lib=json.loads(Path(model).read_text());self.bank=self.lib['resident_matrix'];self.states=[]
        self.log=tempfile.TemporaryFile();self.p=subprocess.Popen([str(Path(worker).resolve()),str(Path(kernel).resolve()),str(Path(model).resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log,env=dict(os.environ,STRUCTURAL_MATRIX_LAYOUT='resident',STRUCTURAL_RESIDENT_SCOPE=scope_mode))
        if self.read(4)!=b'RDC2':raise ValueError('resident worker not selected')
    def read(self,n):
        data=b''
        while len(data)<n:
            x=self.p.stdout.read(n-len(data))
            if not x:
                self.log.seek(0);raise RuntimeError(self.log.read().decode(errors='replace')[:1000])
            data+=x
        return data
    def exchange(self,updates,requests,stale=False,scopes=None):
        v=self.bank['vocabulary_count'];patches=[]
        for row,sort,features in updates:
            new=set(features)|{v+sort}
            if row==len(self.states):self.states.append({'version':0,'sort':sort,'features':set(),'tokens':set()})
            old=self.states[row]
            if new==old['tokens']:continue
            edits=sorted((x,0) for x in old['tokens']-new)+sorted((x,1) for x in new-old['tokens'])
            old.update(version=old['version']+1,sort=sort,features=set(features),tokens=new)
            patches.append([row,old['version'],sort,len(edits)]+[z for pair in edits for z in pair])
        words=[len(requests),len(self.states),len(patches)]+[z for p in patches for z in p]
        if scopes is None:
            scopes={row:[i for i,s in enumerate(self.bank['model_sorts']) if s==self.states[row]['sort']] for row,_ in requests}
        words += [len(scopes)]+[z for row,heads in sorted(scopes.items()) for z in [row,len(heads),*heads]]
        words += [z for row,m in requests for z in (row,m,self.states[row]['version']-int(stale))]
        payload=struct.pack('<'+'I'*len(words),*words)
        self.p.stdin.write(struct.pack('<I',len(payload))+payload);self.p.stdin.flush()
        n,dt=struct.unpack('<Id',self.read(12));assert n==len(requests)
        matrix=struct.unpack('<10Q',self.read(80));resident=struct.unpack('<9Q',self.read(72))
        result=np.frombuffer(self.read(n*4),dtype='<f4')
        expected=np.array([reference(self.lib,self.bank['sorts'][self.states[row]['sort']],sorted(self.states[row]['features']))[m] for row,m in requests])
        err=float(np.max(np.abs(result-expected))) if n else 0
        assert err<2e-5,err
        assert np.all(np.isfinite(result));self.last_scores=result.copy()
        return {'scores':n,'max_error':err,'threshold_disagreements':int(np.sum((result>=.5)!=(expected>=.5))),
                'matrix':list(matrix),'resident':list(resident),'upload_bytes':len(payload)+4,'download_bytes':164+n*4,'device_seconds':dt}
    def close(self):
        if self.p.poll() is None:
            self.p.stdin.write(struct.pack('<I',0));self.p.stdin.flush();self.p.wait(timeout=30)
        self.p.stdin.close();self.p.stdout.close();self.log.close()

def check(worker,kernel,model):
    session=Session(worker,kernel,model);b=session.bank;v=b['vocabulary_count'];models=session.lib['models'];out=[]
    bysort=[[i for i,m in enumerate(models) if b['model_sorts'][i]==s] for s in range(len(b['sorts']))]
    rng=np.random.default_rng(401);updates=[];req=[]
    try:
        for row in range(19):
            s=row%len(bysort);features=np.flatnonzero(rng.random(v)<.3).tolist();updates.append((row,s,features));req.append((row,bysort[s][0]))
        out.append(session.exchange(updates,req))
        all_heads=[(row,m) for row,state in enumerate(session.states) for m in bysort[state['sort']]]
        reuse=session.exchange([],all_heads);assert reuse['matrix'][3:6]==[0,0,0];assert reuse['resident'][3]==len(set(all_heads));assert reuse['resident'][4]>0;out.append(reuse)
        repeat=session.exchange([],all_heads);assert repeat['resident'][4]==0;assert repeat['matrix'][5]==0;out.append(repeat)
        updates=[]
        for row in range(0,19,3):
            st=session.states[row];features=st['features']^{row%v};updates.append((row,st['sort'],sorted(features)))
        for row in range(19,73):
            s=row%len(bysort);updates.append((row,s,np.flatnonzero(rng.random(v)<.5).tolist()))
        req=[(row,m) for row in range(73) for m in bysort[row%len(bysort)]]
        out.append(session.exchange(updates,req))
        out.append(session.exchange([],req))
        try:session.exchange([],[(0,bysort[0][0])],stale=True)
        except RuntimeError:pass
        else:raise AssertionError('stale request was accepted')
        assert session.p.wait(timeout=30)!=0
    finally:session.close()
    # Different exact consumer groups, new consumers without feature changes,
    # partial refresh, then request a previously computed but now stale head.
    scoped=Session(worker,kernel,model)
    try:
        s=max(range(len(bysort)),key=lambda s:sum(not b['constant_models'][m] for m in bysort[s]))
        hs=[m for m in bysort[s] if not b['constant_models'][m]][:3];assert len(hs)==3
        first=scoped.exchange([(0,s,[0,2,3]),(1,s,[1,3])],[(0,hs[0]),(1,hs[0])],scopes={0:hs[:2],1:hs[:1]})
        assert first['resident'][5]==4 and first['matrix'][4]==64 # shared tile beats 128 padded slots
        reuse=scoped.exchange([],[(0,hs[1])],scopes={0:hs[:2]});assert reuse['resident'][4]==1 and reuse['matrix'][5]==0
        late=scoped.exchange([],[(0,hs[2])],scopes={0:hs});assert late['resident'][5]==1
        partial=scoped.exchange([(0,s,[2,4])],[(0,hs[0])],scopes={0:hs[:1]});assert partial['resident'][5]==1
        stale_head=scoped.exchange([],[(0,hs[1])],scopes={0:hs[:2]});assert stale_head['resident'][5]==1 and stale_head['resident'][3]==0
        # Direct demand stays legal even when omitted from publisher scope.
        direct=scoped.exchange([],[(1,hs[2])],scopes={1:[]});assert direct['resident'][5]==1
        out += [first,reuse,late,partial,stale_head,direct]
    finally:scoped.close()
    control=Session(worker,kernel,model,scope_mode='sort')
    try:
        first=control.exchange([(0,s,[0,2,3])],[(0,hs[0])],scopes={})
        assert first['resident'][5]==len([m for m in bysort[s] if not b['constant_models'][m]])
        reuse=control.exchange([],[(0,hs[1])],scopes={});assert reuse['matrix'][5]==0
        out += [first,reuse]
    finally:control.close()
    # The real model may have no summary masks. Exercise the same weights with
    # an explicit per-head exclusion to check both numerator and normalization.
    masked=json.loads(Path(model).read_text());target=next(i for i,c in enumerate(b['constant_models']) if not c)
    masked['models'][target]['excluded_tokens']=[0,1,2]
    with tempfile.TemporaryDirectory() as temp:
        path=Path(temp)/'masked.json';path.write_text(json.dumps(lower(masked)))
        test=Session(worker,kernel,path)
        try:
            s=b['model_sorts'][target]
            a=test.exchange([(0,s,[3,4])],[(0,target)]);before=test.last_scores.copy()
            extra=test.exchange([(0,s,[0,1,2,3,4])],[(0,target)]);after=test.last_scores.copy()
            assert np.array_equal(before,after),'excluded features leaked through normalization'
        finally:test.close()
    return {'schema':'resident-ridge-numeric-check/v2','consumer_scopes_and_late_heads':True,'excluded_feature_invariance':True,'scores':sum(r['scores'] for r in out),
            'max_abs_error':max(r['max_error'] for r in out),'threshold_disagreements':sum(r['threshold_disagreements'] for r in out),
            'stale_version_rejected':True,'growth_preserves_prior_contexts':True,'frames':out,
            'scope':'Old FP16-coordinate ridge reference; approximate predictions, never matching proofs. Not a performance benchmark.'}

def main():
    p=argparse.ArgumentParser()
    for x in ['worker','kernel','model','output']:p.add_argument('--'+x,type=Path,required=True)
    a=p.parse_args();result=check(a.worker,a.kernel,a.model);a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
if __name__=='__main__':main()
