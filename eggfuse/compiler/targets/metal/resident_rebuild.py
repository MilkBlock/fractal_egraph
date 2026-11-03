"""Reuse the stable duplicate/merge algorithm inside the resident device loop.

Parallel scans preserve physical row order. Custom merge callbacks remain in
that order; their lookup cost is not disguised as parallel work.
"""
import re

NAMES=('rebuild_start','rebuild_canonical','rebuild_insert','rebuild_assign',
       'merge_scan','merge_offsets','merge_scatter','rebuild_merge',
       'compact_scan','compact_offsets','compact_copy','compact_store')

def lower(source, index_source=None):
    result=[]
    for name in NAMES:
        match=re.search(r'kernel void '+name+r'\((.*?)\)\{',source,re.S)
        if match is None:raise ValueError('rebuild kernel missing: '+name)
        end=start=match.end();depth=1
        while depth:
            if source[end]=='{':depth+=1
            if source[end]=='}':depth-=1
            end+=1
        body=source[start:end-1]
        args=match[1].replace('KERNEL_ARGS','BC_ARGS,device long*scratch[[buffer(15)]],uint2 point[[thread_position_in_grid]]')
        args=args.replace(',uint group[[threadgroup_position_in_grid]]','')
        pre='''
 uint instance=point.y,lane=point.x,group=lane/256;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device long*r=all_rows+instance*GC_ROW_WORDS;
 // LOCAL_POINTERS
 device uint*p=parent;device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||stats[10]!=6)return;
'''
        if name.startswith('compact_'):pre+='if(control[0]!=0)return;\n'
        else:pre+='if(control[0]!=1)return;\n'
        if name=='rebuild_canonical':
            body=body.replace('r[at]=v;r[lane*STRIDE+2]=m.epoch;', 'atomic_store_explicit((device atomic_uint*)(r+RC_VERSION+t),m.epoch,memory_order_relaxed);rc_fact_dirty(r,lane);r[at]=v;r[lane*STRIDE+2]=m.epoch;')
        if name=='rebuild_merge':
            # Preserve callback order, but suspend before a lookup would rebuild
            # the whole key index on its calling thread.
            body=body.replace('if(PARALLEL_MERGE_INDEX&&', 'if(BC_RESUME_MERGE_INDEX&&')
            body=body.replace('  r[i*STRIDE+1]=', '''  if(r[INDEX_OFFSET]==long(m.unions)+1){
   long key[COLS]={0};for(uint c=0;c<WIDTH[t]-1;c++)key[c]=r[j*STRIDE+4+c];
   if(row_lookup(r,p,m,t,key)==j)r[INDEX_OFFSET]=0;
  }
  rc_dirty(r,t,m.epoch);rc_fact_dirty(r,i);r[i*STRIDE+1]=''')
        if name=='compact_offsets':body=body.replace('(CAPACITY+255)/256','(m.count+255)/256')
        if name=='compact_copy':body=body.replace('matches[to*STRIDE+c]','scratch[(instance*batchCfg.z+to)*STRIDE+c]')
        if name=='compact_store':
            body='''
 if(lane<control[1]){
  if(control[1]!=control[2])for(uint c=0;c<STRIDE;c++)r[lane*STRIDE+c]=scratch[(instance*batchCfg.z+lane)*STRIDE+c];
  r[RC_FACT_ROW+uint(r[lane*STRIDE+3])]=lane+1;
 }
 if(!lane){m.count=control[1];r[INDEX_OFFSET]=0;r[QUERY_DIRTY]=1;}
'''
        result.append(f'kernel void resident_{name}({args}){{'+pre+body+'}')
        if name=='rebuild_merge' and index_source is not None:
            result.append('kernel void resident_rebuild_resume('+args+'){'+pre.replace('control[0]!=1','control[0]!=2')+body+'}')
    result.append('''
kernel void resident_rebuild_gate(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x||all_meta[instance].error)return;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));if(stats[10]!=6)return;
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL;
 if(control[0]==1){
  atomic_fetch_max_explicit(dispatch+BC_RB_HASH,(HASH_CAP+255)/256,memory_order_relaxed);
  atomic_fetch_max_explicit(dispatch+BC_RB_ROWS,(all_meta[instance].count+255)/256,memory_order_relaxed);
  atomic_fetch_max_explicit(dispatch+BC_RB_ONE,1u,memory_order_relaxed);
 }
}
kernel void resident_compact_gate(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x||all_meta[instance].error)return;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));if(stats[10]!=6)return;
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL;
 if(control[0]==0){
  // No compacted rows have been written yet. Growth resumes this exact state,
  // preserving every match/context and without executing any RHS again.
  if(all_meta[instance].count>batchCfg.z){all_meta[instance].error=38;return;}
  atomic_fetch_max_explicit(dispatch+BC_RB_COMPACT,(all_meta[instance].count+255)/256,memory_order_relaxed);
  atomic_fetch_max_explicit(dispatch+BC_RB_FINISH,1u,memory_order_relaxed);
 }
}
kernel void resident_rebuild_finish(BC_ARGS,uint2 point[[thread_position_in_grid]]){
 uint instance=point.y;if(instance>=cfg.x||point.x||all_meta[instance].error)return;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));if(stats[10]!=6)return;
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL;if(control[0])return;
 bc_finish_run(BC_VALUES,instance,bool(control[21]));
}
''')
    if index_source is not None:
        # The cursor remains at the first unexecuted callback. Index refresh
        # reads canonical keys but neither replays effects nor starts rebuild.
        for name in ('row_index_context','row_index_clear','row_index_parallel','row_index_ready'):
            m=re.search(r'kernel void '+name+r'\((.*?)\)\{',index_source,re.S)
            if m is None:raise ValueError('merge-index kernel missing: '+name)
            start=end=m.end();depth=1
            while depth:
                depth+=(index_source[end]=='{')-(index_source[end]=='}');end+=1
            body=index_source[start:end-1]
            args=m[1].replace('KERNEL_ARGS','BC_ARGS,uint2 point[[thread_position_in_grid]]')
            pre="""
 uint instance=point.y,lane=point.x;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device long*r=all_rows+instance*GC_ROW_WORDS;
 // LOCAL_POINTERS
 device uint*p=parent;device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||stats[10]!=6||control[0]!=2)return;
"""
            if name=='row_index_clear':body+='if(!lane)r[RC_INDEX_TOUCH]=0;'
            if name=='row_index_parallel':
                old='if(atomic_compare_exchange_weak_explicit(index+2*h,&expected,lane+1,memory_order_relaxed,memory_order_relaxed))return;'
                if body.count(old)!=1:raise ValueError('merge-index insertion changed')
                body=body.replace(old,'if(atomic_compare_exchange_weak_explicit(index+2*h,&expected,lane+1,memory_order_relaxed,memory_order_relaxed)){uint at=atomic_fetch_add_explicit((device atomic_uint*)(r+RC_INDEX_TOUCH),1u,memory_order_relaxed);r[RC_INDEX_TOUCH+1+at]=h;return;}')
            if name=='row_index_ready':body+='if(!lane&&!m.error){stats[56]++;stats[57]+=m.count;}'
            result.append('kernel void resident_merge_'+name+'('+args+'){'+pre+body+'}')
        result.append("""
kernel void resident_merge_index_gate(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x||all_meta[instance].error)return;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL;
 if(stats[10]!=6||control[0]!=2)return;
 // A callback may have appended rows before suspending. Size from the current
 // arena, not the row count frozen when duplicate grouping began.
 atomic_fetch_max_explicit(dispatch+BC_RB_HASH,(HASH_CAP+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_RB_ROWS,(all_meta[instance].count+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_RB_ONE,1u,memory_order_relaxed);
}
""")
    return '\n'.join(result)
