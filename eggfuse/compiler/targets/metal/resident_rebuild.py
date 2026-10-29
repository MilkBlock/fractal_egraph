"""Reuse the stable duplicate/merge algorithm inside the resident device loop.

Parallel scans preserve physical row order. Custom merge callbacks remain in
that order; their lookup cost is not disguised as parallel work.
"""
import re

NAMES=('rebuild_start','rebuild_canonical','rebuild_insert','rebuild_assign',
       'merge_scan','merge_offsets','merge_scatter','rebuild_merge',
       'compact_scan','compact_offsets','compact_copy','compact_store')

def lower(source):
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
            # Callback evaluation uses the original ordered scalar lookup. A
            # later optimization may split its index refresh without reordering.
            body=re.sub(r'  if\(PARALLEL_MERGE_INDEX.*?return;\}\n','',body)
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
    return '\n'.join(result)
