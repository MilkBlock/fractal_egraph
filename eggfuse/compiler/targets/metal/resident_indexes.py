"""Bind the existing stable query-index kernels to the resident arena ABI."""
import re

NAMES=('row_index_context','row_index_clear','row_index_parallel','row_index_ready',
       'query_row_scan','query_row_offsets','query_pairs','query_radix_scan',
       'query_radix_offsets','query_radix_scatter','query_link_sorted','query_table_time')


def lower(source):
    result=[]
    for name in NAMES:
        # The resident compiler uses the existing four-bit radix implementation.
        match=re.search(r'kernel void '+name+r'\((.*?)\)\{',source,re.S)
        if match is None:raise ValueError('query-index kernel missing: '+name)
        start=match.end();end=start;depth=1
        while depth:
            if end>=len(source):raise ValueError('unterminated query-index kernel: '+name)
            if source[end]=='{':depth+=1
            if source[end]=='}':depth-=1
            end+=1
        body=source[start:end-1]
        args=match[1].replace('KERNEL_ARGS','BC_ARGS,uint2 point[[thread_position_in_grid]]')
        args=args.replace(',device ulong*alternate[[buffer(14)]]','').replace(',uint group[[threadgroup_position_in_grid]]','')
        args=args.replace('constant uint&bit[[buffer(13)]]','constant uint&bit[[buffer(14)]]')
        pre='''
 uint instance=point.y,lane=point.x,group=lane/256;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device long*r=all_rows+instance*GC_ROW_WORDS;device long*old=r;
 // LOCAL_POINTERS
 device uint*p=parent;device ulong*alternate=(device ulong*)(matches+GC_QUERY_ALTERNATE);
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||stats[10]!=5)return;
'''
        if name=='query_row_offsets':
            body+='atomic_fetch_max_explicit(dispatch+BC_INDEX_PAIRS,(n+255)/256,memory_order_relaxed);'
        if name=='query_link_sorted':
            body=body.replace('qheads[h]=e;', '{qheads[h]=e;qheads[Q_TOUCH+2+h]=control[17];}')
        if name.startswith('row_index_'):pre+='if(!control[20])return;\n'
        if name=='row_index_clear':body+='if(!lane)r[RC_INDEX_TOUCH]=0;'
        if name=='row_index_parallel':
            before='if(atomic_compare_exchange_weak_explicit(index+2*h,&expected,lane+1,memory_order_relaxed,memory_order_relaxed))return;'
            if body.count(before)!=1:raise ValueError('row index CAS body changed')
            body=body.replace(before,'''if(atomic_compare_exchange_weak_explicit(index+2*h,&expected,lane+1,memory_order_relaxed,memory_order_relaxed)){
 uint at=atomic_fetch_add_explicit((device atomic_uint*)(r+RC_INDEX_TOUCH),1u,memory_order_relaxed);r[RC_INDEX_TOUCH+1+at]=h;return;}''')
        result.append(f'kernel void resident_{name}({args}){{'+pre+body+'}')
    result.append('''
kernel void resident_query_clear(BC_ARGS,uint2 point[[thread_position_in_grid]]){
 uint instance=point.y,lane=point.x;if(instance>=cfg.x)return;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));if(all_meta[instance].error||stats[10]!=5)return;
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL;
 device uint*qheads=all_uint+instance*GC_UINT_WORDS+GC_QHEADS;
 if(!control[0])return;
 if(lane<TABLE_COUNT)qheads[2*QBUCKETS+1+lane]=0;
 if(control[18]&&lane<QBUCKETS)qheads[Q_TOUCH+2+lane]=0;
 if(!lane)qheads[Q_TOUCH]=control[17];
}
''')
    return '\n'.join(result)
