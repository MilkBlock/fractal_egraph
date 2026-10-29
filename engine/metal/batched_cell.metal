// Three device stages share one resident context/effect matrix. The host queues
// a fixed quantum without reading matches or deciding logical stage transitions.
#define BC_ARGS device long*all_rows[[buffer(0)]],device uint*all_uint[[buffer(1)]],device long*all_long[[buffer(2)]],device Meta*all_meta[[buffer(3)]],device uint*all_state[[buffer(4)]],device long*code[[buffer(5)]],device const uint*tape[[buffer(6)]],device uint*all_history[[buffer(7)]],device long*all_markers[[buffer(8)]],constant uint4&cfg[[buffer(9)]],device long*all_batch[[buffer(10)]],constant uint4&batchCfg[[buffer(11)]],device atomic_uint*dispatch[[buffer(12)]],device ulong*hashes[[buffer(13)]],constant uint4&writeCfg[[buffer(16)]],device uint*all_write_uint[[buffer(17)]],device long*all_write_long[[buffer(18)]]
#define BC_INPUTS device long*all_rows,device uint*all_uint,device long*all_long,device Meta*all_meta,device uint*all_state,device long*code,device const uint*tape,device uint*all_history,device long*all_markers,constant uint4&cfg,device long*all_batch,constant uint4&batchCfg,device atomic_uint*dispatch,device ulong*hashes,constant uint4&writeCfg,device uint*all_write_uint,device long*all_write_long
#define BC_VALUES all_rows,all_uint,all_long,all_meta,all_state,code,tape,all_history,all_markers,cfg,all_batch,batchCfg,dispatch,hashes,writeCfg,all_write_uint,all_write_long
__attribute__((noinline)) void bc_finish_run(BC_INPUTS,uint instance,bool continuation);
bool bc_requires_sort(uint set){return QUERY_PARALLEL[set]||!QUERY_PATCH_SORTED;}
uint bc_order_offset(uint count,bool parallel){
 if(!BC_PARALLEL_SORT||!parallel||count<=BC_SMALL_SORT)return 0;
 uint at=0;for(uint span=BC_SORT_TILE;span<count;span*=2)at=MATCHCAP-at;return at;
}
__attribute__((noinline)) void bc_prepare_queries(BC_INPUTS,uint instance,bool reset,bool indexed){
 device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device long*r=all_rows+instance*GC_ROW_WORDS;device long*old=r;
 // LOCAL_POINTERS
 device uint*p=parent;device float*context_results=(device float*)contexts;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));context_info=(device uint*)stats;
 if(indexed)qheads[2*QBUCKETS]=m.count;
 else build_query_index(old,reset?0:qheads[2*QBUCKETS],m.count,reset,qheads,qlinks,qprev);
 if(!reset&&r[QUERY_DIRTY]==2)patch_query_index(old,qheads,qlinks,qprev);
 for(uint j=0;j<uint(r[QUERY_LOG]);j++)r[QUERY_OLD_TIME+uint(r[QUERY_CHANGED+j])]=0;
 if(reset)s[5]++;else s[6]++;r[QUERY_LOG]=0;r[QUERY_LOG+1]=m.count;r[QUERY_DIRTY]=0;ensure_row_index(r,p,m);
 if(m.rounds==1)for(uint row=0;row<m.count;row++)r[RC_FACT_ROW+uint(r[row*STRIDE+3])]=row+1;
 rc_compact(contexts,r);bc_reindex_certificates(stats,r,m.count,m.unions,ulong(r[RC_VISIBILITY]));s[7]=1;
 uint set=tape[4*s[0]+1];
 if(QUERY_PARALLEL[set]){
  switch(set){// CACHE_PREPARE
   default:m.error=20;return;
  }
  if(m.error)return;stats[10]=3;
  atomic_fetch_max_explicit(dispatch+6,max(1u,(m.count+127)/128),memory_order_relaxed);return;
 }
 switch(set){// LOCAL_QUERIES
  default:m.error=20;return;
 }
 if(m.error)return;stats[10]=4;
}
kernel void batched_cell_prepare_finish(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x)return;device Meta&m=all_meta[instance];
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||stats[10]!=5)return;
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL;
 bc_prepare_queries(BC_VALUES,instance,bool(control[19]),bool(control[0]));
}
kernel void batched_cell_prepare(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device uint*stack=s+8;device long*r=all_rows+instance*GC_ROW_WORDS;device long*old=r;
 // LOCAL_POINTERS
 device uint*p=parent;device float*context_results=(device float*)contexts;
 if(m.rounds==0)for(uint row=0;row<m.count;row++){uint t=uint(r[row*STRIDE]);r[RC_VERSION+t]=max(r[RC_VERSION+t],r[row*STRIDE+2]);r[RC_FACT_ROW+uint(r[row*STRIDE+3])]=row+1;r[RC_FACT_REV+uint(r[row*STRIDE+3])]=1;}
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));bool grew=stats[9]!=long(batchCfg.x);stats[9]=batchCfg.x;
 if(m.error==37){m.error=0;contexts[14]=1;rc_compact(contexts,r);}
 // Checkpoint diagnostic: stop only between complete logical Runs. A pending
 // block continuation finishes before the gate can close.
 if(batchCfg.y && s[3]>=batchCfg.y && stats[10]!=2){stats[10]=0;return;}
 if(stats[10]==6||stats[10]==8||stats[10]==9||stats[10]==10)return;
 if(stats[10]==2){if(grew)bc_reindex_certificates(stats,r,m.count,m.unions,ulong(r[RC_VISIBILITY]));atomic_fetch_max_explicit(dispatch,uint(stats[11]),memory_order_relaxed);return;}stats[10]=0;
 context_info=(device uint*)stats;
 while(!m.error&&!s[2]){
  uint pc=s[0],op=tape[4*pc],arg=tape[4*pc+1];
  if(op==1||op==2){if(op==1&&!arg){s[0]=tape[pc*4+2];continue;}uint at=8*s[1]++;stack[at]=op;stack[at+1]=arg;stack[at+2]=pc+1;stack[at+3]=tape[pc*4+2];for(uint j=4;j<8;j++)stack[at+j]=0;stack[at+7]=tape[pc*4+3];s[0]++;continue;}
  if(op==3){if(!s[1]){m.error=20;return;}uint at=8*(s[1]-1),changed=stack[at+5];stack[at+4]++;stack[at+6]|=changed;
   if(changed&&(stack[at]==2||stack[at+4]<stack[at+1])){stack[at+5]=0;s[0]=stack[at+2];}else{s[0]=stack[at+3];s[1]--;if(s[1])stack[8*(s[1]-1)+5]|=stack[at+6];}continue;}
  if(op==6){
   // Single-arena private publication has a typed completeness proof. A stop
   // inside it uses the complete original Run path, never a partial patch.
   if(BC_SCHEDULED&&cfg.x==1&&!batchCfg.w){stats[10]=8;return;}
   s[0]++;continue;
  }
  if(op==4){s[2]=1;return;}
  if(op==5){bool found=false;for(uint i=0;i<m.count;i++)found|=r[i*STRIDE]==arg&&r[i*STRIDE+1]==0;if(!found){m.error=8;return;}s[0]++;continue;}
  if(op!=0){m.error=20;return;}
  if(s[3]>=cfg.z){s[4]=1;return;}
  if(!s[7]&&m.epoch==0xffffffff){m.error=6;return;}
  uint set=arg;stats[13]=m.unions;if(!s[7]){m.epoch++;m.rounds++;m.changed=0;}m.matched=0;r[ROW_WORDS+1]=m.epoch;
  if(BC_SKIP_QUIESCENT&&bc_quiescent(set,r,watermarks)){
   bc_finish_run(BC_VALUES,instance,false);
   // Audit emits a fingerprint at every Run. Ordinary execution can walk all
   // unchanged source control on-device without another query/publication pass.
   if(stats[14]||(batchCfg.y&&s[3]>=batchCfg.y))return;
   continue;
  }
  bool reset=m.rounds==1||r[QUERY_DIRTY]==1;
  if(BC_PARALLEL_INDEXES){
   stats[10]=5;control[19]=reset;control[0]=reset&&m.count>256;
   if(control[0]){
    control[20]=r[INDEX_OFFSET]!=long(m.unions)+1;
    if(control[20]){
     atomic_fetch_max_explicit(dispatch+BC_INDEX_HASH_ROWS,(m.count+255)/256,memory_order_relaxed);
     atomic_fetch_max_explicit(dispatch+BC_INDEX_HASH_BUCKETS,(HASH_CAP+255)/256,memory_order_relaxed);
    }
    control[17]=qheads[Q_TOUCH]+1;
    // The radix histogram reuses heads. Preserve/restore its epoch outside that
    // scratch and invalidate all tags if a very wide layout overlaps them.
    control[18]=control[17]==0||((m.count*QSTRIDE+255)/256+1)*16>=Q_TOUCH;
    if(!control[17])control[17]=1;
    atomic_fetch_max_explicit(dispatch+BC_INDEX_ROWS,(m.count+255)/256,memory_order_relaxed);
    atomic_fetch_max_explicit(dispatch+BC_INDEX_GATE,1u,memory_order_relaxed);
    atomic_fetch_max_explicit(dispatch+BC_INDEX_CLEAR,(max(uint(TABLE_COUNT),control[18]?uint(QBUCKETS):1u)+255)/256,memory_order_relaxed);
   }
   return;
  }
  bc_prepare_queries(BC_VALUES,instance,reset,false);return;
 }
}
kernel void batched_cell_match(BC_ARGS,uint2 group[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 uint instance=group.y,lane=group.x*128+tid;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device long*r=all_rows+instance*GC_ROW_WORDS;device long*old=r;
 // LOCAL_POINTERS
 device uint*p=parent;device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 context_info=(device uint*)stats;device float*context_results=(device float*)contexts;
 if(m.error||stats[10]!=3)return;uint set=tape[4*s[0]+1];
 switch(set){// PARALLEL_QUERIES
  default:m.error=20;return;
 }
}
// Sorting operates only on immutable ordinals. Every binding and its evidence
// stay correlated in the original match buffer. No CPU count readback is needed.
kernel void batched_cell_sort_local(BC_ARGS,uint2 group[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 uint instance=group.y,lane=group.x*BC_SORT_TILE+tid;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||(stats[10]!=3&&stats[10]!=4))return;
 device uint*order=all_uint+instance*GC_UINT_WORDS+GC_MATCH_ORDER;
 if(stats[10]==4&&QUERY_PATCH_SORTED){if(lane<m.matched)order[lane]=lane;return;}
 device long*matches=all_long+instance*GC_LONG_WORDS+GC_MATCHES;
 threadgroup uint ids[BC_SORT_TILE];ids[tid]=lane<m.matched?lane:0xffffffff;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint k=2;k<=BC_SORT_TILE;k*=2)for(uint d=k/2;d;d/=2){
  uint other=tid^d,a=ids[tid],b=ids[other];bool low=((tid&k)==0)==(tid<other),before=match_before(a,b,matches);
  threadgroup_barrier(mem_flags::mem_threadgroup);ids[tid]=low?(before?a:b):(before?b:a);threadgroup_barrier(mem_flags::mem_threadgroup);
 }
 if(lane<m.matched)order[lane]=ids[tid];
}
kernel void batched_cell_sort_merge(BC_ARGS,constant uint4&sortCfg[[buffer(14)]],uint2 group[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 uint instance=group.y,lane=group.x*128+tid;if(instance>=cfg.x)return;device Meta&m=all_meta[instance];
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||(stats[10]!=3&&(stats[10]!=4||QUERY_PATCH_SORTED))||lane>=m.matched||sortCfg.x>=m.matched)return;
 device uint*order=all_uint+instance*GC_UINT_WORDS+GC_MATCH_ORDER;
 device uint*input=order+sortCfg.y;device uint*output=order+sortCfg.z;
 device long*matches=all_long+instance*GC_LONG_WORDS+GC_MATCHES;
 uint span=sortCfg.x,base=(lane/(2*span))*2*span,mid=min(base+span,m.matched),end=min(base+2*span,m.matched);
 bool left=lane<mid;uint own=left?base:mid,other=left?mid:base,lo=other,hi=left?end:mid,id=input[lane];
 // The comparator is total (including ordinal tie-break), so ranks are unique.
 while(lo<hi){uint at=lo+(hi-lo)/2;if(match_before(input[at],id,matches))lo=at+1;else hi=at;}
 output[base+(lane-own)+(lo-other)]=id;
}
kernel void batched_cell_pack(BC_ARGS,constant uint&packStep[[buffer(14)]],uint instance[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 if(instance>=cfg.x)return;device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device uint*stack=s+8;device long*r=all_rows+instance*GC_ROW_WORDS;
 // LOCAL_POINTERS
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||(stats[10]!=3&&stats[10]!=4))return;
 if(m.matched>batchCfg.x){if(!tid)m.error=34;return;}
 if(BC_PARALLEL_SORT&&packStep==0&&m.matched>BC_SMALL_SORT){
  if(tid)return;
  atomic_fetch_max_explicit(dispatch+9,(m.matched+BC_SORT_TILE-1)/BC_SORT_TILE,memory_order_relaxed);
  uint pass=0;for(uint span=BC_SORT_TILE;span<MATCHCAP;span*=2,pass++)if((stats[10]==3||!QUERY_PATCH_SORTED)&&m.matched>span)
   atomic_fetch_max_explicit(dispatch+12+3*pass,(m.matched+127)/128,memory_order_relaxed);
  return;
 }
 // Sort immutable match ordinals, never reorder the correlated binding itself.
 // The total key is source rule/variant/anchor/within-anchor ordinal. Atomic
 // arrival order is not a semantic tie breaker for distinct bindings.
 if(BC_PARALLEL_SORT&&m.matched<=BC_SMALL_SORT){
  // Four ordinals per lane fit in 4 KiB of threadgroup memory. Finish small
  // batches in the same kernel that publishes their descriptors.
  if(m.matched>1&&(stats[10]==3||!QUERY_PATCH_SORTED)){
   threadgroup uint ids[BC_SMALL_SORT];uint limit=1;while(limit<m.matched)limit*=2;
   for(uint i=tid;i<limit;i+=BC_SORT_TILE)ids[i]=i<m.matched?i:0xffffffff;
   threadgroup_barrier(mem_flags::mem_threadgroup);
   for(uint k=2;k<=limit;k*=2)for(uint d=k/2;d;d/=2){
    uint values[BC_SMALL_SORT/BC_SORT_TILE];
    for(uint i=tid;i<limit;i+=BC_SORT_TILE){uint other=i^d,a=ids[i],b=ids[other];bool low=((i&k)==0)==(i<other),before=match_before(a,b,matches);values[i/BC_SORT_TILE]=low?(before?a:b):(before?b:a);}
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for(uint i=tid;i<limit;i+=BC_SORT_TILE)ids[i]=values[i/BC_SORT_TILE];
    threadgroup_barrier(mem_flags::mem_threadgroup);
   }
   for(uint i=tid;i<m.matched;i+=BC_SORT_TILE)match_order[i]=ids[i];
  }else for(uint i=tid;i<m.matched;i+=BC_SORT_TILE)match_order[i]=i;
  threadgroup_barrier(mem_flags::mem_device);
  if(tid)return;
 }else if(BC_PARALLEL_SORT){if(tid)return;match_order+=bc_order_offset(m.matched,stats[10]==3||!QUERY_PATCH_SORTED);}
 else {if(tid)return;
 for(uint i=0;i<m.matched;i++)match_order[i]=i;
 if(stats[10]==3||!QUERY_PATCH_SORTED)for(uint span=1;span<m.matched;span*=2){
  for(uint start=0;start<m.matched;start+=2*span){uint mid=min(start+span,m.matched),end=min(start+2*span,m.matched),a=start,b=mid;
   for(uint i=start;i<end;i++)match_order[MATCHCAP+i]=(b==end||(a<mid&&match_before(match_order[a],match_order[b],matches)))?match_order[a++]:match_order[b++];
  }
  for(uint i=0;i<m.matched;i++)match_order[i]=match_order[MATCHCAP+i];
 }
 }
  if(m.error)return;
  uint tiles=0,start=0,certificates=uint(stats[15]);device long*meta=stats+BC_STATS;
  while(start<m.matched){uint rule=uint(matches[match_order[start]*MSTRIDE]),count=0;
   while(start+count<m.matched&&count<BC_TILE&&uint(matches[match_order[start+count]*MSTRIDE])==rule)count++;
   meta[3*tiles]=start;meta[3*tiles+1]=count;meta[3*tiles+2]=bc_select(rule,stack,s[1]);certificates+=count*bc_certificate_bound(int(meta[3*tiles+2]));tiles++;start+=count;
   stats[6]=max(stats[6],long(count));
  }
  if(certificates>batchCfg.x){m.error=34;return;}
  stats[11]=tiles;stats[0]+=tiles;stats[10]=1;s[7]=0;
  atomic_fetch_max_explicit(dispatch,tiles,memory_order_relaxed);
}

kernel void batched_cell_evaluate(BC_ARGS,uint2 group[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 uint instance=group.y,tile=group.x;if(instance>=cfg.x)return;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if((stats[10]!=1&&stats[10]!=2)||tile>=uint(stats[11]))return;
 device long*meta=stats+BC_STATS;uint start=uint(meta[3*tile]),count=uint(meta[3*tile+1]);int plan=int(meta[3*tile+2]);
 device long*data=meta+3*batchCfg.x+start*BC_STRIDE;
 device long*matches=all_long+instance*GC_LONG_WORDS+GC_MATCHES;
 device uint*match_order=all_uint+instance*GC_UINT_WORDS+GC_MATCH_ORDER;
 device uint*s=all_state+instance*GC_STATE_WORDS;
 match_order+=bc_order_offset(all_meta[instance].matched,bc_requires_sort(tape[4*s[0]+1]));
 if(plan<0){
  if(tid<count&&stats[10]==1){
   uint binding=match_order[start+tid],rule=uint(matches[binding*MSTRIDE]);
   device long*r=all_rows+instance*GC_ROW_WORDS;device long*o=data+tid*BC_STRIDE;o[BC_FAST]=0;
   if(NOOP_PREFLIGHT&&NOOP_RULE[rule]){o[BC_FAST]=actions_stably_empty(rule,binding,code,matches,r);rc_count(stats,29);}
  }
  return;
 }
 threadgroup float as[256],bs[256],cs[256];
 if(tid<count&&stats[10]==1)bc_prepare(plan,match_order[start+tid],matches,data+tid*BC_STRIDE);
 threadgroup_barrier(mem_flags::mem_device|mem_flags::mem_threadgroup);
 bc_matrix(plan,data,count,tid,as,bs,cs);
 threadgroup_barrier(mem_flags::mem_device|mem_flags::mem_threadgroup);
 if(tid<count)bc_evaluate(plan,data+tid*BC_STRIDE);
}
__attribute__((noinline)) void bc_finish_run(BC_INPUTS,uint instance,bool continuation){
 device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device uint*stack=s+8;device long*r=all_rows+instance*GC_ROW_WORDS;
 // LOCAL_POINTERS
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 uint pc=s[0],set=tape[4*pc+1];
 // One finite pass over saved continuations after a block changed identities.
 // Completed steps are tickets; no seed or effect is replayed.
 if(continuation){
  // Rebuild compacts physical rows. Per-Run constructor predictions are keyed
  // by the old row positions and cannot survive into this continuation block.
  r[QUERY_LOG]=0;r[QUERY_LOG+1]=m.count;stats[10]=2;return;
 }
 for(uint j=SET_START[set];j<SET_START[set+1];j++)watermarks[SET_RULES[j]]=m.epoch-1;
 device uint*h=all_history+(instance*cfg.z+s[3])*12;
 h[0]=set;h[1]=m.changed;h[2]=m.matched;h[3]=m.epoch;h[4]=m.count;h[5]=m.unions;h[6]=pc;h[7]=s[1];
 h[8]=0;h[9]=0;h[10]=0;h[11]=0;
 if(stats[14]){uint words=m.count*STRIDE+m.next_id+m.next_fact+uint(r[ROW_WORDS]);atomic_fetch_max_explicit(dispatch+3,(words+1023)/1024,memory_order_relaxed);}
 s[3]++;s[0]++;stats[10]=0;if(s[1])stack[8*(s[1]-1)+5]|=m.changed;
}
kernel void batched_cell_publish(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device uint*stack=s+8;device long*r=all_rows+instance*GC_ROW_WORDS;device long*old=r;
 // LOCAL_POINTERS
 device uint*p=parent;device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||(stats[10]!=1&&stats[10]!=2&&stats[10]!=10))return;device long*meta=stats+BC_STATS;device long*data=meta+3*batchCfg.x;
 match_order+=bc_order_offset(m.matched,bc_requires_sort(tape[4*s[0]+1]));
 uint pc=s[0],set=tape[4*pc+1];bool resumedPublisher=stats[10]==10;
 if(!resumedPublisher){control[26]=stats[10]==2;control[27]=0;ulong progress=ulong(stats[1]+stats[2]);control[28]=uint(progress);control[29]=uint(progress>>32);p[IDCAP]=1;p[IDCAP+1]=0;if(control[26])stats[16]++;stats[21]=0;}
 bool resuming=control[26];long progressed=long(ulong(control[28])|(ulong(control[29])<<32));
 for(uint tile=0;tile<uint(stats[11])&&!m.error;tile++){
  uint start=uint(meta[3*tile]),count=uint(meta[3*tile+1]);int plan=int(meta[3*tile+2]);
  if(start+count<=control[27])continue;
  if(BC_PARALLEL_CONSTRUCTORS&&!resuming&&plan<0&&BATCH_RULE[uint(matches[match_order[start]*MSTRIDE])]){
   uint end=start;
   for(uint t=tile;t<uint(stats[11]);t++){
    uint next=uint(meta[3*t]),nr=uint(matches[match_order[next]*MSTRIDE]);
    if(meta[3*t+2]>=0||!BATCH_RULE[nr])break;end=next+uint(meta[3*t+1]);
   }
   if(end-start>=256){ensure_row_index(r,p,m);control[23]=start;control[24]=end;stats[10]=9;return;}
  }
  for(uint j=0;j<count&&!m.error;j++){
   uint binding=match_order[start+j];device long*o=data+(start+j)*BC_STRIDE;
   if(plan>=0){stats[7]+=o[BC_FAST]!=0;bc_publish(plan,binding,matches,o,r,p,fp,m,stats);stats[21]+=bc_waiting(plan,o);}
   else if(!resuming){if(NOOP_PREFLIGHT&&o[BC_FAST])stats[28]++;else execute_actions(uint(matches[binding*MSTRIDE]),binding,code,registers,matches,r,p,fp,m);stats[5]++;}
  }
 }
 if(m.error)return;
 bool pending=p[IDCAP+1]!=0;uint unionsBefore=m.unions;
 p[IDCAP]=0;for(uint j=0;j<p[IDCAP+1];j++)unite_now(p,p[IDCAP+2+2*j],p[IDCAP+3+2*j],m);
 p[IDCAP+1]=0;if(m.unions!=unionsBefore){
  stats[17]++;
  if(BC_PARALLEL_REBUILD){control[0]=1;control[21]=pending&&stats[21]&&stats[1]+stats[2]>progressed;stats[10]=6;return;}
  rebuild(r,p,fp,m,qheads,qlinks);
 }
 if(m.error)return;
 bc_finish_run(BC_VALUES,instance,pending&&stats[21]&&stats[1]+stats[2]>progressed);
}

// Position-salted two-lane integer fingerprints. Bounded fixed work per lane;
// the final reduction is deterministic and independent of execution order.
#define BC_HASH_CHUNKS ((CAPACITY*STRIDE+2*IDCAP+SET_WORDS+1023)/1024)
ulong bc_mix(ulong x){x=(x^(x>>30))*0xbf58476d1ce4e5b9UL;x=(x^(x>>27))*0x94d049bb133111ebUL;return x^(x>>31);}
kernel void batched_cell_fingerprint(BC_ARGS,uint2 group[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 uint instance=group.y;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||!stats[14]||stats[22]==s[3])return;
 device long*r=all_rows+instance*GC_ROW_WORDS;
 // LOCAL_POINTERS
 uint nrows=m.count*STRIDE,nparent=m.next_id,nfact=m.next_fact,nset=uint(r[ROW_WORDS]);
 uint total=nrows+nparent+nfact+nset;ulong a=0,b=0;
 for(uint i=group.x*1024+tid;i<min(total,(group.x+1)*1024);i+=128){
  uint at=i;ulong x;
  if(at<nrows)x=ulong(r[at]);else if((at-=nrows)<nparent)x=parent[at];else if((at-=nparent)<nfact)x=fp[at];else{x=ulong(r[ROW_WORDS+at-nfact]);}
  a+=bc_mix(x^(ulong(i)*0x9e3779b97f4a7c15UL));b^=bc_mix(x+ulong(i)*0xd6e8feb86659fd93UL);
 }
 threadgroup ulong aa[128],bb[128];aa[tid]=a;bb[tid]=b;threadgroup_barrier(mem_flags::mem_threadgroup);
 if(!tid){for(uint i=1;i<128;i++){a+=aa[i];b^=bb[i];}uint at=(instance*BC_HASH_CHUNKS+group.x)*2;hashes[at]=a;hashes[at+1]=b;}
}
kernel void batched_cell_fingerprint_finish(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x)return;device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||!stats[14]||stats[22]==s[3])return;
 device long*r=all_rows+instance*GC_ROW_WORDS;uint total=m.count*STRIDE+m.next_id+m.next_fact+uint(r[ROW_WORDS]);ulong a=0,b=0;
 for(uint i=0;i<(total+1023)/1024;i++){uint at=(instance*BC_HASH_CHUNKS+i)*2;a+=hashes[at];b^=hashes[at+1];}
 device uint*h=all_history+(instance*cfg.z+s[3]-1)*12;h[8]=uint(a);h[9]=uint(a>>32);h[10]=uint(b);h[11]=uint(b>>32);stats[22]=s[3];
}
