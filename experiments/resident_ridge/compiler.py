"""Compile certified whole-Cell frames; keep the existing ridge solver intact."""
import hashlib,json,sys
from pathlib import Path
import numpy as np
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'experiments/structural_key_memory'))
import train
from memory import fingerprint,codes


def compile_library(lib):
    states=lib['states'];steps=lib['steps'];names=[]
    if not states or not steps:raise ValueError('empty transfer family')
    for step in steps:
        if not step['bake'].get('native_complete') or not step['bake'].get('delta_replay_verified') or step['bake'].get('last_step_updated') is not False:
            raise ValueError('incomplete native closure/delta certificate')
        for p in step['parameters']:
            if p['sort']!='Math':raise ValueError('prototype accepts opaque Math parameters only')
            if p['name'] not in names:names.append(p['name'])
    if len(names)>31:raise ValueError('ready-mask ABI requires at most 31 parameters')
    sizes=[0]+[len(s['values']) for s in states];slots=max(sizes);consumed=[[]]
    for state in states:
        if state['scope']['semantics']!='positive-constructor-equality/v1' or any(v.get('literal') is not None or v['sort']!='Math' for v in state['values']):
            raise ValueError('unsupported closure semantics/value domain')
        consumed.append([[slot,names.index(k[6:])] for k,slot in state['ports'].items() if k.startswith('input:')])
    transitions=[];successors={}
    for i,step in enumerate(steps):
        source=0 if step['source'] is None else step['source']+1;target=step['target']+1
        if not (0<=source<len(sizes) and 1<=target<len(sizes)):raise ValueError('state index out of bounds')
        if source in successors:raise ValueError('branching within one context needs a multi-page ABI; cannot select one branch silently')
        successors[source]=target
        params=[names.index(p['name']) for p in step['parameters']]
        if len(step['input_to_target'])!=sizes[source]+len(params):raise ValueError('incomplete inherited binding map')
        if len(step['input_partition'])!=len(step['input_to_target']):raise ValueError('partition/mapping arity mismatch')
        if len(set(step['input_partition'][:sizes[source]]))!=sizes[source]:raise ValueError('source classes contradict the certified canonical frame')
        if any(not 0<=i<sizes[target] for i in step['input_to_target']):raise ValueError('target slot out of bounds')
        for i in range(len(step['input_partition'])):
            for j in range(i):
                if (step['input_partition'][i]==step['input_partition'][j])!=(step['input_to_target'][i]==step['input_to_target'][j]):raise ValueError('inherited equality map inconsistent')
        for slot,param in consumed[source]:
            if states[target-1]['ports'].get('input:'+names[param])!=step['input_to_target'][slot]:raise ValueError('inherited named port moved incorrectly')
        for j,param in enumerate(params):
            if states[target-1]['ports'].get('input:'+names[param])!=step['input_to_target'][sizes[source]+j]:raise ValueError('new named port moved incorrectly')
        inverse=[-1]*sizes[target]
        for j,to in enumerate(step['input_to_target']):
            if inverse[to]<0:inverse[to]=j
        transitions.append(dict(source=source,target=target,parameters=params,partition=step['input_partition'],mapping=inverse,
            new_enodes=step['emitted_rows'],unions=step['union_actions'],name=step['name']))
    for s in successors:
        seen=set();p=s
        while p in successors:
            if p in seen:raise ValueError('cyclic transfer graph needs an explicit recurrence contract')
            seen.add(p);p=successors[p]
    features=[(0,s,0,0) for s in range(len(sizes))]+[(1,p,0,0) for p in range(len(names))]+[(2,i,j,0) for i in range(len(names)) for j in range(i)]
    for t in transitions:
        source=t['source'];new=set(t['parameters']);old={i for _,i in consumed[source]}
        features.append((3,source,sum(1<<i for i in new),0))
        for i in sorted(old|new):
            for j in sorted(old|new):
                if j<i and (i in new or j in new):features.append((4,source,i,j))
    features=list(dict.fromkeys(features))
    hashes=np.array([fingerprint(('resident-ridge',*f)) for f in features],dtype=np.uint64)
    return dict(schema='resident-ridge/v1',parameter_names=names,sizes=sizes,slots=slots,consumed=consumed,transitions=transitions,
        sweeps=len(states),features=features,codes=codes(hashes,256).astype(np.int32).tolist(),
        fresh_base=1<<30,fresh_stride=sum(sizes),state_offsets=np.cumsum([0]+sizes[:-1]).tolist(),
        library_sha256=hashlib.sha256(json.dumps(lib,sort_keys=True).encode()).hexdigest())


def active_features(p,state,ready,params):
    def on(f):
        kind,a,b,c=f
        if kind==0:return state==a
        if kind==1:return bool(ready>>a&1)
        if kind==2:return bool(ready>>a&1 and ready>>b&1 and params[a]==params[b])
        if kind==3:return state==a and ready&b==b
        if kind==4:return state==a and bool(ready>>b&1 and ready>>c&1 and params[b]==params[c])
        raise ValueError('unknown feature')
    return [int(on(f)) for f in p['features']]


def frame_for(p,state,params,row=0):
    f=[p['fresh_base']+row*p['fresh_stride']+p['state_offsets'][state]+s for s in range(p['sizes'][state])]
    for slot,parameter in p['consumed'][state]:f[slot]=params[parameter]
    return f+[0]*(p['slots']-len(f))


def valid_source(p,state,ready,params,frame):
    return len(set(frame[:p['sizes'][state]]))==p['sizes'][state] and all(ready>>i&1 and frame[s]==params[i] for s,i in p['consumed'][state])


def guard(p,t,state,ready,params,frame):
    if t['source']!=state or not all(ready>>i&1 for i in t['parameters']):return 0
    if not valid_source(p,state,ready,params,frame):return 2
    inputs=frame[:p['sizes'][state]]+[params[i] for i in t['parameters']]
    if any((inputs[i]==inputs[j])!=(t['partition'][i]==t['partition'][j]) for i in range(len(inputs)) for j in range(i)):return 2
    return 1


def partitions(n):
    if n==0:yield [];return
    for prefix in partitions(n-1):
        for i in range(max(prefix,default=-1)+2):yield prefix+[i]


def fit(p):
    examples=[];xs=[];ys=[]
    code=np.array(p['codes'],dtype=np.int32)
    for state in range(len(p['sizes'])):
        for part in partitions(len(p['parameter_names'])):
            for ready in range(1<<len(part)):
                f=frame_for(p,state,part)
                if not valid_source(p,state,ready,part,f):continue
                a=active_features(p,state,ready,part);n=sum(a)
                xs.append((np.array(a)@code/train.divisor(n)).astype(np.float16))
                ys.append([int(guard(p,t,state,ready,part,f)==1) for t in p['transitions']]);examples.append((state,part,ready))
    x=np.array(xs,dtype=np.float16);y=np.array(ys,dtype=np.uint8);ids=np.arange(len(x),dtype=np.int64)
    held=np.array([fingerprint(e)%7==0 for e in examples])
    g=dict(payload=y,probes=ids);c=dict(x=x,domain=ids,query=ids)
    predicted,_,model=train.fit(g,c,held)  # unchanged frequency-weighted lambda=1 solver
    p['ridge']=dict(width=256,weights=model['w'].view(np.uint16).tolist(),bias=model['bias'].tolist(),scale=256,
        solver='experiments/structural_key_memory/train.py::fit',solver_sha256=hashlib.sha256(Path(train.__file__).read_bytes()).hexdigest())
    p['training']=dict(samples=len(y),held_out=int(held.sum()),columns=y.shape[1],
        held_out_threshold_errors=int(np.count_nonzero((predicted[held]>=.5)!=y[held])),
        scope='Finite compiled state/ready/alias domain; fresh numeric input labels are not feature coordinates; not unseen Cell-family generalization.')
    return p


def expand_graph(p,lib,record):
    rows=[];params=record['parameters'];state=record['state'];frame=record['frame']
    for value in sorted(set(params)):rows.append(dict(op='Var',args=[['i64',str(value)]],result=str(value)))
    if state:
        for r in lib['states'][state-1]['rows']:
            rows.append(dict(op=r['op'],args=[['Math',str(frame[a])] for a in r['args']],result=str(frame[r['result']])))
    return rows
