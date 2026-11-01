"""Physical key DAG layout for the shared structural-key contract.

The first Metal implementation evaluates key allocation/comparison in the ordered
executor. It never substitutes hash ordering or CPU-computed winners.
"""
import json
from pathlib import Path
HEADER=8
OPS={'cell-key-min':0,'cell-key-anchor':1,'cell-key-cmp':2,'cell-key-prefer-left':3,'cell-key-select':4,
     'cell-key-base-i64':5,'cell-key-base-bool':6,'cell-key-base-Unit':7,'cell-key-base-String':8,'cell-key-base-f64':9,'cell-key-decode':11}

def enabled(core):return any(s['kind']=='structural_key' for t in core['tables'] for s in t['schema'])

def layout(core,capacity,strings):
    if not enabled(core):return None
    spec=core.get('structural_key')
    if not isinstance(spec,dict):raise ValueError('structural key type requires its source runtime declaration')
    nodes=spec['nodes'];labels=[n['label'] for n in nodes]
    if len({n['primitive'] for n in nodes})!=len(nodes):raise ValueError('duplicate key primitive')
    for n in nodes:
        if not n['primitive'].startswith('cell-key-node-') or not isinstance(n['arity'],int) or n['arity']<0:raise ValueError('invalid key declaration')
    string_labels={x['value']:x['label'] for x in spec.get('string_labels',[])}
    if any(s not in string_labels for s in strings):raise ValueError('missing exact native structural string labels')
    labels+=['\0input-anchor','i64:','bool:false','bool:true','Unit:()','f64:']+[string_labels[s] for s in strings]
    decoded=[]
    for text in strings:
        ns=[]
        def decode(x):
            if not isinstance(x,list) or len(x)!=3 or type(x[0]) is not bool or not isinstance(x[1],str) or not isinstance(x[2],list):raise ValueError('invalid key literal')
            if x[0] and (x[1]!='\0input-anchor' or len(x[2])!=1):raise ValueError('invalid key anchor')
            children=[decode(c) for c in x[2]];i=len(ns);ns.append({'anchor':x[0],'label':x[1],'children':children});return i
        try:root=decode(json.loads(text));decoded.append({'nodes':ns,'root':root})
        except (ValueError,TypeError,RecursionError):decoded.append(None)
    for w in decoded:
        if w:
            for n in w['nodes']:
                if n['label'] not in labels:labels.append(n['label'])
    words=max(1024,capacity*64)
    return {'schema':'eggfuse-key-arena/v1','nodes':nodes,'labels':labels,'string_count':len(strings),
            'storage_words':words,'scratch_words':3*(words//5+1),'execution':'ordered GPU key operations; exact shared DAG semantics',
            'decoded':decoded,'max_label_bytes':max([32]+[len(s.encode()) for s in labels])}

def encode_keys(core,info):
    arena=[HEADER,0,0,0,0,0,0,0];intern={}
    def add(anchor,label,children):
        key=(anchor,label,tuple(children))
        if key in intern:return intern[key]
        data=label.encode();packed=[int.from_bytes(data[i:i+8].ljust(8,b'\0'),'little',signed=True) for i in range(0,len(data),8)]
        height=0 if anchor else max([arena[c+2]+1 for c in children]+[0]);at=len(arena)
        arena.extend([5+len(packed)+len(children),int(anchor),height,len(data),len(children),*packed,*children]);arena[0]=len(arena);intern[key]=at;return at
    def wire(w):
        ids=[]
        for n in w['nodes']:
            if any(i<0 or i>=len(ids) for i in n['children']):raise ValueError('invalid initial key DAG')
            if n['anchor'] and (n['label']!='\0input-anchor' or len(n['children'])!=1):raise ValueError('invalid initial key anchor')
            ids.append(add(n['anchor'],n['label'],[ids[i] for i in n['children']]))
        return ids[w['root']]
    for r in core['initial']:
        for x in r['values']:
            if 'key' in x:wire(x['key'])
    return arena,wire

def operation(call,info):
    name=call['primitive']
    if not name.startswith('cell-key-'):return None
    if info is None:raise ValueError('key operation without key contract')
    if name in OPS:return OPS[name],0
    for i,n in enumerate(info['nodes']):
        if name==n['primitive']:
            if call['inputs']!=['CellStructuralKey']*n['arity'] or call['output']!='CellStructuralKey':raise ValueError('key node signature differs')
            return 10,i
    raise ValueError('unsupported structural key primitive '+name)

def shader(info,offset):
    if info is None:return ''
    info['offset_words']=offset
    data=[];starts=[0]
    for s in info['labels']:data.extend(s.encode());starts.append(len(data))
    constants=f'#define SK_BASE (ROW_WORDS+{offset})\n#define SK_WORDS {info["storage_words"]}\n#define SK_MAX_LABEL {info["max_label_bytes"]}\n#define SK_NODE_COUNT {len(info["nodes"])}\n'
    constants+='constant uchar SK_LABELS[]={'+','.join(map(str,data or [0]))+'};\nconstant uint SK_LABEL_START[]={'+','.join(map(str,starts))+'};\n'
    code=[];offsets=[];roots=[];counts=[];arity=1
    for w in info['decoded']:
        offsets.append(len(code));roots.append(w['root'] if w else 0);counts.append(len(w['nodes']) if w else 0)
        if w:
            for n in w['nodes']:
                code += [int(n['anchor']),info['labels'].index(n['label']),len(n['children']),*n['children']];arity=max(arity,len(n['children']))
    constants+=f'#define SK_DECODE_ARITY {arity}\n'
    for name,x in [('SK_DECODE_CODE',code),('SK_DECODE_START',offsets),('SK_DECODE_ROOT',roots),('SK_DECODE_COUNT',counts)]:constants+='constant uint '+name+'[]={'+','.join(map(str,x or [0]))+'};\n'
    return constants+(Path(__file__).resolve().parents[4]/'engine/metal/structural_keys.metal').read_text()
