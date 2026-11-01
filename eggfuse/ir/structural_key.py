import json
from pathlib import Path
ABI=json.loads((Path(__file__).resolve().parents[2]/'engine/structural-key/abi.json').read_text())
def validate_core(core):
    spec=core.get('structural_key',{})
    if any(s['kind']=='structural_key' for t in core['tables'] for s in t['schema']) and spec.get('abi')!=ABI:raise ValueError('structural-key ABI differs; recompile the source through the shared frontend')
    def walk(x):
        if isinstance(x,list):
            for v in x:walk(v)
        elif isinstance(x,dict):
            if all(k in x for k in ('primitive','inputs','output')) and x['primitive'].startswith('cell-key-'):
                sig=ABI['primitives'].get(x['primitive'])
                if sig:
                    ty=x['output'] if sig[1]=='$T' else sig[1]
                    good=x['output']==ty and x['inputs']==[ty if t=='$T' else t for t in sig[0]]
                else:
                    n=next((n for n in spec.get('nodes',[]) if n['primitive']==x['primitive']),None)
                    good=n is not None and x['inputs']==['CellStructuralKey']*n['arity'] and x['output']=='CellStructuralKey'
                if not good:raise ValueError('structural-key primitive signature differs: '+x['primitive'])
            for v in x.values():walk(v)
    walk(core)

"""Exact transport and merge contracts for structural witness keys."""
def key_token(wire):
    nodes=wire['nodes'];root=wire['root'];done={};intern={};canonical=[];work=[(root,False)]
    while work:
        i,ready=work.pop()
        if i in done:continue
        if not isinstance(i,int) or not 0<=i<len(nodes):raise ValueError('invalid key root')
        n=nodes[i]
        if type(n['anchor']) is not bool or not isinstance(n['label'],str):raise ValueError('invalid key node type')
        if any(not isinstance(c,int) or not 0<=c<i for c in n['children']):raise ValueError('invalid key DAG ordering')
        if n['anchor'] and (n['label']!='\0input-anchor' or len(n['children'])!=1):raise ValueError('invalid key anchor')
        if not ready:
            work.append((i,True));work.extend((c,False) for c in reversed(n['children']));continue
        value=(bool(n['anchor']),n['label'],tuple(done[c] for c in n['children']))
        if value not in intern:intern[value]=len(canonical);canonical.append(value)
        done[i]=intern[value]
    return tuple(canonical),done[root]

def arena_key(arena,root):
    nodes=[];ids={};heights={};at=8
    while at<len(arena) and at<arena[0]:
        words,anchor,height,length,arity=arena[at:at+5];packed=(length+7)//8
        if words!=5+packed+arity or at+words>len(arena):raise ValueError('invalid GPU key record')
        raw=b''.join((v&((1<<64)-1)).to_bytes(8,'little') for v in arena[at+5:at+5+packed])[:length]
        child_handles=arena[at+5+packed:at+words]
        expected_height=0 if anchor else max([heights[c]+1 for c in child_handles]+[0])
        if anchor not in (0,1) or height!=expected_height:raise ValueError('GPU key height/anchor differs')
        heights[at]=height
        children=[ids[c] for c in child_handles]
        ids[at]=len(nodes);nodes.append({'anchor':bool(anchor),'label':raw.decode(),'children':children});at+=words
    if root not in ids:raise ValueError('invalid GPU key handle')
    return {'nodes':nodes,'root':ids[root]}

def merge_contracts(core):
    result=[];types={t['name']:t['kind'] for t in core['tables']}
    for table in core['tables']:
        expr=table.get('merge_expr');call=(expr or {}).get('call',{});op=call.get('primitive');args=(expr or {}).get('args',[])
        kind='ordered';permission='original submission order'
        if table['kind']=='constructor':kind='congruence';permission='exact equality propagation'
        elif expr is None:kind='unique';permission='conflicting values remain an error'
        elif op in ('min','max','or','and','cell-key-min') and len(args)==2 and sorted(x.get('var','') for x in args)==['new','old']:
            kind='key-min' if op=='cell-key-min' else 'lattice';permission='associative commutative idempotent on declared value domain'
        elif op=='cell-key-select' and len(args)==3 and args[0].get('var')=='old' and args[1].get('var')=='new' and args[2].get('call',{}).get('primitive')=='cell-key-prefer-left':kind='ranked-selection';permission='ordered reads; reassociation needs frozen port versions and equivalent equal-rank candidates'
        reads=[];constructs=[]
        def visit(x):
            if not isinstance(x,dict):return
            if 'table' in x.get('call',{}):
                name=x['call']['table'];(constructs if types[name]=='constructor' else reads).append(name)
            for a in x.get('args',[]):visit(a)
        visit(expr)
        result.append({'table':table['name'],'kind':kind,'expression':expr,'reads':sorted(set(reads)),'constructs':sorted(set(constructs)),'equal_inputs':'return_old' if 'table' in call else 'evaluate','permission':permission})
    return {'schema':'eggfuse-merge-contracts/v1','tables':result}
