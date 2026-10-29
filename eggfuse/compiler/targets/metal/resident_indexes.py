"""Bind the existing stable query-index kernels to the resident arena ABI."""
import re

NAMES=('row_index_context','row_index_clear','row_index_parallel','row_index_ready',
       'query_row_scan','query_row_offsets','query_pairs','query_radix_scan',
       'query_radix_offsets','query_radix_scatter','query_link_sorted','query_table_time')


def lower(source,hierarchical=True):
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
            if hierarchical:body+='atomic_fetch_max_explicit(dispatch+BC_INDEX_HIST,((n+65535)/65536)*16,memory_order_relaxed);'
        if hierarchical and name=='query_radix_offsets':
            body='''
 if(lane>=16||!control[0])return;uint groups=(control[7]+255)/256,tiles=(groups+255)/256,n=0;
 for(uint tile=0;tile<tiles;tile++){uint at=tile*16+lane,v=radix_sums[at];radix_sums[at]=n;n+=v;}
 qheads[groups*16+lane]=n;
 if(!lane&&bit+1==QUERY_RADIX_PASSES){control[8]=(2*QBUCKETS+255)/256;control[9]=1;control[10]=1;}
'''
        if hierarchical and name=='query_radix_scatter':
            before='qheads[(lane/256)*16+digit]+qlinks[lane]'
            if body.count(before)!=1:raise ValueError('radix scatter prefix changed')
            body=body.replace(before,before+'+radix_sums[(lane/65536)*16+digit]')
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
    if hierarchical:result.append('''
kernel void resident_query_radix_blocks(BC_ARGS,uint2 group[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]],uint sid[[simdgroup_index_in_threadgroup]],uint slane[[thread_index_in_simdgroup]],uint swidth[[threads_per_simdgroup]]){
 uint instance=group.y,tile=group.x/16,digit=group.x%16;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||stats[10]!=5)return;
 // LOCAL_POINTERS
 uint groups=(control[7]+255)/256;if(!control[0]||tile*256>=groups)return;
 uint at=tile*256+tid,value=at<groups?qheads[at*16+digit]:0;
 uint prefix=simd_prefix_exclusive_sum(value),sum=simd_sum(value);
 threadgroup uint sums[32];if(!slane)sums[sid]=sum;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint i=0;i<sid;i++)prefix+=sums[i];if(at<groups)qheads[at*16+digit]=prefix;
 if(!tid){uint total=0;for(uint i=0;i<256/swidth;i++)total+=sums[i];radix_sums[tile*16+digit]=total;}
}
''')
    return '\n'.join(result)
