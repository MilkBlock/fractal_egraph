import hashlib
import json
from pathlib import Path


def idv(v):return v['Eq'][1]
def num(v):return int(v['Base'][1])


def load_snapshot(path):
    saved=json.loads(path.read_text());rows=saved['rows']
    loop={'loops':[],'contexts':[],'types':[],'constants':[]}
    branch={'ifs':[],'args':[],'gets':[],'state_values':[]}
    cmp={'pairs':[],'smaller':[]};state_bases=set();state_types=set();typed=[]
    for op,a,v,hidden in rows:
        if hidden:continue
        if op=='DoWhile':loop['loops'].append([idv(v),idv(a[0]),idv(a[1])])
        elif op=='ContextOf':loop['contexts'].append([idv(a[0]),idv(a[1])])
        elif op=='HasArgType':loop['types'].append([idv(a[0]),idv(a[1])])
        elif op=='Const':loop['constants'].append([idv(v),idv(a[0]),idv(a[1]),idv(a[2])])
        elif op=='If':branch['ifs'].append([idv(v),*[idv(x) for x in a]])
        elif op=='Arg':branch['args'].append([idv(v),idv(a[0]),idv(a[1])])
        elif op=='Get':branch['gets'].append([idv(a[0]),num(a[1]),idv(v)])
        elif op=='StateT':state_bases.add(idv(v))
        elif op=='Base':typed.append((idv(a[0]),idv(v)))
        elif op=='TCPair':cmp['pairs'].append([idv(v),idv(a[0]),num(a[1])])
        elif op=='Smaller':cmp['smaller'].append([idv(v),idv(a[0]),idv(a[1])])
    state_types={v for b,v in typed if b in state_bases}
    branch['state_values']=list({idv(a[0]) for op,a,v,h in rows if op=='HasType' and not h and idv(a[1]) in state_types})
    return loop,branch,cmp,{'path':str(path.resolve()),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
                          'scope':'final canonical visible tables of saved same-submit CPU Pass 3; not runtime timing'}


def fixture():
    # Nonfunctional ports, arbitrary IDs/values, missing context, repeated
    # constant values under different output types. No recurrence or affine law.
    return {'loops':[[100,10,11],[101,12,11],[102,13,14]],
            'contexts':[[10,0],[10,1],[11,2],[12,1],[13,0]],
            'types':[[10,7],[10,9],[12,7],[13,5]],
            'constants':[[200,333,55,2],[201,-12,56,2],[202,333,57,2],[203,1000003,77,3]]}


def unstructured_fixture():
    import random
    rng=random.Random(741)
    keys=[(i*37)^0x579 for i in range(64)]
    data={'loops':[],'contexts':[],'types':[],'constants':[]}
    for i in range(16):
        inp,body=1000+2*i,1001+2*i
        data['loops'].append([9000+i,inp,body])
        data['contexts'] += [[inp,c] for c in rng.sample(keys,2)]
        data['contexts'] += [[body,c] for c in rng.sample(keys,5)]
        data['types'] += [[inp,t] for t in rng.sample(range(7),2)]
    seen=set()
    for i in range(96):
        c,t=i%11,2000+i%8;ctx=keys[i%64]
        while (c,t,ctx) in seen:ctx=rng.choice(keys)
        seen.add((c,t,ctx));data['constants'].append([1000000+i,c,t,ctx])
    return data


def crossed_if_fixture():
    return {'ifs':[[100,0,10,11,12]],'args':[[20,1,0],[21,2,1],[22,2,2],[23,1,3]],
            'gets':[[11,1000003,70],[12,1000003,71],[20,4,70],[21,9,70],[22,4,71],[23,9,71]],
            'state_values':[70]}
