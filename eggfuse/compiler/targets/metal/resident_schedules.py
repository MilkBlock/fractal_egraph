"""Attach certified private schedule regions to the resident engine.

Use the existing fixed-carrier proof and control compiler. Repeat remains a
finite opcode in the private GPU program; it is not replaced by saturation.
"""
import copy
from . import scheduled_matrix, matrix_sat


def description(core, program, region):
    normalized=copy.deepcopy(core)
    for rs in normalized['rulesets']:
        for rule in rs['rules']:rule.setdefault('source_set',rs['name'])
    extended, info=scheduled_matrix.prepare(normalized)
    # A single Run does not need a private schedule/context setup.
    info['programs']=[p for p in info['programs'] if len(p['control']['instructions'])>2]
    for i,p in enumerate(info['programs']):p['index']=i
    if not info['programs']:return None,None,None
    identities={tuple(r['identity']):r['id'] for r in region['rules']}
    sets=[[identities[(r.get('source_set',rs['name']),r['name'])] for r in rs['rules']] for rs in extended['rulesets']]
    tids={t['name']:i for i,t in enumerate(core['tables'])};strings={s:i for i,s in enumerate(program['strings'])}
    code,matrix=matrix_sat.lower(extended,sets,tids,strings,True)
    empty,_=matrix_sat.lower(core,program['set_rule_ids'],tids,strings,False)
    info['matrix']=matrix
    info['all_sets']=[r['name'] for r in extended['rulesets']]
    info['execution']='private GPU state and original finite/nested control; publish at proved region exit'
    info['coverage']='complete fixed structural carrier and positive opaque unary predicates; other effects remain resident residual'
    return info,code,empty


def prepare(core,program,region):
    info,code,empty=description(core,program,region)
    if info is not None:
        if program['shader'].count(empty)!=1:raise ValueError('matrix lowering anchor changed')
        program['shader']=program['shader'].replace(empty,code)
    return info


def metal(source):
    # Private stages get cumulative fingerprints of their new fact coordinates.
    # They are not falsely reported as hashes of already-published global rows.
    source=source.replace('uint at=8*c[1]', 'uint at=12*c[1]').replace('history[8*(c[1]-1)+2]', 'history[12*(c[1]-1)+2]')
    source=source.replace('history[at+7]=c[13]?stack[8*(c[13]-1)+4]:0;', 'history[at+7]=c[13]?stack[8*(c[13]-1)+4]:0;history[at+8]=0x534d;history[at+9]=c[16];history[at+10]=c[17];history[at+11]=c[15];')
    needle='if(!old){birth[f]=c[1]+1;atomic_fetch_add_explicit((device atomic_uint*)(c+5),1u,memory_order_relaxed);}'
    replacement='''if(!old){birth[f]=c[1]+1;atomic_fetch_add_explicit((device atomic_uint*)(c+5),1u,memory_order_relaxed);
 uint x=(f+1)*0x9e3779b9u^(c[1]+1)*0x85ebca6bu;x=(x^(x>>16))*0x7feb352du;x^=x>>15;
 atomic_fetch_add_explicit((device atomic_uint*)(c+16),x,memory_order_relaxed);
 atomic_fetch_xor_explicit((device atomic_uint*)(c+17),x*0x846ca68bu,memory_order_relaxed); }'''
    if source.count(needle)!=1:raise ValueError('scheduled predicate publisher changed')
    source=source.replace(needle,replacement)
    source+='''
kernel void resident_schedule_index(MS_ARGS,uint lane[[thread_position_in_grid]]){
 if(c[4]||lane>=cfg[5])return;uint row=c[8]+lane,t=uint(rows[row*STRIDE]);
 rows[RC_FACT_ROW+uint(rows[row*STRIDE+3])]=row+1;
 atomic_fetch_max_explicit((device atomic_uint*)(rows+RC_VERSION+t),uint(rows[row*STRIDE+2]),memory_order_relaxed);
 if(!lane){rows[INDEX_OFFSET]=0;rows[QUERY_DIRTY]=1;rows[QUERY_LOG]=0;rows[QUERY_LOG+1]=c[8]+cfg[5];}
}
'''
    return source
