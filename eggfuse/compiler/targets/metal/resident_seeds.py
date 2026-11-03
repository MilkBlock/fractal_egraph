"""Compact exact seminaive seed rows before entering large query functions."""

def lower(program):
    cases=[];ground=[]
    for si,rules in enumerate(program['set_rule_ids']):
        routes=[r for r in program['query_seed_routes'] if r['rule'] in rules]
        ground.append(int(any(r['table'] is None for r in routes)))
        lines=[]
        for route in routes:
            t=route['table'];rid=route['rule']
            if t is None:continue
            gate='||'.join(f'qheads[2*QBUCKETS+1+{tr}]>watermarks[{rid}]' for tr in route['trigger']) or 'true'
            if route['relation']=='fresh':action=f'seeds[SD_BOUNDS+2*{t}]=min(seeds[SD_BOUNDS+2*{t}],watermarks[{rid}]);'
            elif route['relation']=='old':action=f'seeds[SD_BOUNDS+2*{t}+1]=max(seeds[SD_BOUNDS+2*{t}+1],watermarks[{rid}]);'
            else:action=f'seeds[SD_BOUNDS+2*{t}+1]=0xffffffff;'
            lines.append(f'if({gate}){{{action}}}')
        cases.append(f'case {si}:{{'+''.join(lines)+'break;}')
    return '''
#define SD_BLOCKS (2*CAPACITY)
#define SD_BOUNDS (SD_BLOCKS+(CAPACITY+255)/256)
constant uint SD_GROUNDED[]={'''+','.join(map(str,ground))+'''};
void bc_seed_bounds(uint set,device uint*seeds,device uint*qheads,device uint*watermarks){
 for(uint t=0;t<TABLE_COUNT;t++){seeds[SD_BOUNDS+2*t]=0xffffffff;seeds[SD_BOUNDS+2*t+1]=0;}
 switch(set){'''+''.join(cases)+'''default:break;}
}
kernel void resident_seed_scan(BC_ARGS,uint2 point[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 uint instance=point.y,lane=point.x,group=lane/256;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||stats[10]!=3)return;
 // LOCAL_POINTERS
 device uint*seeds=seed_rows;device long*r=all_rows+instance*GC_ROW_WORDS;
 uint live=0;if(lane<m.count&&r[lane*STRIDE+1]==0){uint t=uint(r[lane*STRIDE]),stamp=uint(r[lane*STRIDE+2]);live=stamp>seeds[SD_BOUNDS+2*t]||stamp<=seeds[SD_BOUNDS+2*t+1];}
 threadgroup uint offsets[256];offsets[tid]=live;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint step=1;step<256;step*=2){uint add=tid>=step?offsets[tid-step]:0;threadgroup_barrier(mem_flags::mem_threadgroup);offsets[tid]+=add;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<m.count)seeds[lane]=live?offsets[tid]-1:0xffffffff;
 if(tid==255)seeds[SD_BLOCKS+group]=offsets[tid];
}
kernel void resident_seed_offsets(BC_ARGS,uint2 point[[thread_position_in_grid]]){
 uint instance=point.y;if(instance>=cfg.x||point.x)return;
 device Meta&m=all_meta[instance];device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||stats[10]!=3)return;
 // LOCAL_POINTERS
 device uint*seeds=seed_rows;device uint*s=all_state+instance*GC_STATE_WORDS;
 uint count=0;for(uint group=0;group<(m.count+255)/256;group++){uint n=seeds[SD_BLOCKS+group];seeds[SD_BLOCKS+group]=count;count+=n;}
 control[30]=count;uint jobs=count+SD_GROUNDED[tape[4*s[0]+1]];
 atomic_fetch_max_explicit(dispatch+6,(jobs+127)/128,memory_order_relaxed);
 stats[36]+=m.count;stats[37]+=count;stats[38]+=jobs-count;
}
kernel void resident_seed_scatter(BC_ARGS,uint2 point[[thread_position_in_grid]]){
 uint instance=point.y,lane=point.x;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||stats[10]!=3||lane>=m.count)return;
 // LOCAL_POINTERS
 if(seed_rows[lane]!=0xffffffff)seed_rows[CAPACITY+seed_rows[SD_BLOCKS+lane/256]+seed_rows[lane]]=lane;
}
'''
