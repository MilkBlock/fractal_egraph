"""Bind the existing ordered constructor-batch proof to resident Cell execution."""
import re

NAMES=('batch_output_scan','batch_output_offsets','batch_clear','batch_propose',
       'batch_group','batch_scan','batch_reserve','batch_publish','batch_index','batch_ordered')

def lower(source):
    result=['''
#define CB_BLOCK 64
#define CB_LEADER (CB_BLOCK+(writeCfg.x+255)/256)
#define CB_PREFIX (CB_LEADER+writeCfg.x)
#define CB_HASH (CB_PREFIX+writeCfg.x)
#define CB_EFFECT_OFFSET (CB_HASH+2*writeCfg.x)
#define CB_WORDS (CB_EFFECT_OFFSET+writeCfg.x)
''']
    for name in NAMES:
        m=re.search(r'kernel void '+name+r'\((.*?)\)\{',source,re.S)
        if m is None:raise ValueError('constructor kernel missing: '+name)
        end=start=m.end();depth=1
        while depth:
            if source[end]=='{':depth+=1
            if source[end]=='}':depth-=1
            end+=1
        body=source[start:end-1]
        args=m[1].replace('BATCH_ARGS','BC_ARGS,uint2 point[[thread_position_in_grid]]').replace(',uint group[[threadgroup_position_in_grid]]','')
        body=re.sub(r'batch_binding\(([^,]+),bs\)',r'match_order[\1]',body)
        for v in ['BLOCK','LEADER','PREFIX','HASH','EFFECT_OFFSET']:body=body.replace('BATCH_'+v,'CB_'+v)
        if name=='batch_output_scan':
            body=body.replace('count=BATCH_WRITES[uint(matches[binding*MSTRIDE])];', '''if(NOOP_PREFLIGHT&&data[(bs[2]+lane)*BC_STRIDE+BC_FAST])atomic_fetch_add_explicit((device atomic_uint*)(bs+44),1u,memory_order_relaxed);else count=BATCH_WRITES[uint(matches[binding*MSTRIDE])];''')
        if name=='batch_output_offsets':
            before='if(n>MATCHCAP){bs[0]=1;bs[16]=bs[19]=bs[22]=bs[40]=0;return;}'
            if before not in body:raise ValueError('constructor capacity gate changed')
            body=body.replace(before,'if(n>writeCfg.x){bs[43]=n;m.error=40;return;}')
            body+='''atomic_fetch_max_explicit(dispatch+BC_WRITE_EFFECTS,(n+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_WRITE_HASH,(bs[3]+255)/256,memory_order_relaxed);'''
        if name=='batch_propose':body=body.replace('uint binding=', 'if(NOOP_PREFLIGHT&&data[(bs[2]+lane)*BC_STRIDE+BC_FAST])return;uint binding=',1)
        if name=='batch_publish':body+='''r[RC_FACT_ROW+fid]=i+1;rc_fact_dirty(r,i);
 atomic_fetch_max_explicit((device atomic_uint*)(r+RC_VERSION+t),m.epoch,memory_order_relaxed);'''
        if name=='batch_index':
            before='if(atomic_compare_exchange_weak_explicit(index+2*h,&expected,i+1,memory_order_relaxed,memory_order_relaxed))return;'
            if before not in body:raise ValueError('constructor hash publication changed')
            body=body.replace(before,'''if(atomic_compare_exchange_weak_explicit(index+2*h,&expected,i+1,memory_order_relaxed,memory_order_relaxed)){
 uint at=atomic_fetch_add_explicit((device atomic_uint*)(r+RC_INDEX_TOUCH),1u,memory_order_relaxed);r[RC_INDEX_TOUCH+1+at]=h;return;}''')
        if name=='batch_ordered':body=body.replace('(NOOP_PREFLIGHT||EFFECT_DEDUP)&&noop[binding]','NOOP_PREFLIGHT&&data[(bs[2]+i)*BC_STRIDE+BC_FAST]')
        pre='''
 uint instance=point.y,lane=point.x,group=lane/256;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device long*r=all_rows+instance*GC_ROW_WORDS;
 // LOCAL_POINTERS
 device uint*p=parent;device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||stats[10]!=9)return;
 device uint*bs=all_write_uint+instance*CB_WORDS;
 device long*proposals=all_write_long+instance*writeCfg.x*BATCH_STRIDE;
 device long*data=stats+BC_STATS+3*batchCfg.x;
 device uint*s=all_state+instance*GC_STATE_WORDS;
 match_order+=bc_order_offset(m.matched,bc_requires_sort(tape[4*s[0]+1]));
'''
        result.append(f'kernel void resident_{name}({args}){{'+pre+body+'}')
    result.append('''
kernel void resident_write_gate(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x||all_meta[instance].error)return;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));if(stats[10]!=9)return;
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL;
 device uint*bs=all_write_uint+instance*CB_WORDS;
 uint count=control[24]-control[23];
 for(uint i=0;i<64;i++)bs[i]=0;bs[2]=control[23];bs[37]=count;
 if(count>writeCfg.x){bs[43]=count;all_meta[instance].error=40;return;}
 atomic_fetch_max_explicit(dispatch+BC_WRITE_BINDINGS,(count+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_WRITE_ONE,1u,memory_order_relaxed);
}
kernel void resident_write_finish(BC_ARGS,uint2 point[[thread_position_in_grid]]){
 uint instance=point.y;if(instance>=cfg.x||point.x||all_meta[instance].error)return;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));if(stats[10]!=9)return;
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL;
 device uint*bs=all_write_uint+instance*CB_WORDS;
 stats[5]+=bs[37];stats[28]+=bs[44];stats[30]+=bs[9];stats[31]+=bs[10];stats[32]++;stats[33]+=bs[11];
 control[27]=control[24];stats[10]=10;
}
''')
    return '\n'.join(result)
