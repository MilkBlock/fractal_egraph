// Stable radix partition of (bucket, entry) pairs. Input entries are
// compacted in physical-row/column order, so equal buckets retain that order.
kernel void query_row_scan(KERNEL_ARGS,uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 if(m.error||!control[0])return;threadgroup uint sums[256];
 uint n=(lane<m.count&&old[lane*STRIDE+1]==0)?INDEX_COUNT[uint(old[lane*STRIDE])]:0;
 sums[tid]=n;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sums[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sums[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<m.count)qlinks[lane]=sums[tid]-n;
 if(tid==255)qheads[group]=sums[tid];
}
kernel void query_row_offsets(KERNEL_ARGS){
 if(lane||m.error||!control[0])return;uint n=0;
 for(uint b=0;b<(m.count+255)/256;b++){uint v=qheads[b];qheads[b]=n;n+=v;}
 control[7]=n;control[4]=max(1u,(n+255)/256);control[5]=1;control[6]=1;
}
kernel void query_pairs(KERNEL_ARGS){
 if(m.error||!control[0]||lane>=m.count||old[lane*STRIDE+1]!=0)return;
 uint t=uint(old[lane*STRIDE]),base=qheads[lane/256]+qlinks[lane];device ulong*out=(device ulong*)matches;
 uint n=0;
 for(uint c=0;c<WIDTH[t];c++)if(INDEX_COLUMN[t*QSTRIDE+c])out[base+n++]=(ulong(query_hash(t,c,old[lane*STRIDE+4+c]))<<32)|ulong(lane*QSTRIDE+c);
 if(INDEX_COLUMN[t*QSTRIDE+COLS])out[base+n++]=(ulong(HASH_CAP+t)<<32)|ulong(lane*QSTRIDE+COLS);
 if(INDEX_COLUMN[t*QSTRIDE+TIME_COLUMN])out[base+n]=(ulong(query_hash(t,TIME_COLUMN,old[lane*STRIDE+2]))<<32)|ulong(lane*QSTRIDE+TIME_COLUMN);
}
#if QUERY_RADIX_DIGIT_BITS == 4
kernel void query_radix_scan(KERNEL_ARGS,constant uint&bit[[buffer(13)]],device ulong*alternate[[buffer(14)]],uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]],uint sid[[simdgroup_index_in_threadgroup]],uint slane[[thread_index_in_simdgroup]],uint swidth[[threads_per_simdgroup]]){
 if(m.error||!control[0])return;
 threadgroup uint totals[16*256];
 device ulong*input=(bit&1)?alternate:(device ulong*)matches;
 bool live=lane<control[7];uint digit=live?uint((input[lane]>>(32+4*bit))&15):0;
 uint groups=256/swidth,local=0;
 // SIMD prefix sums keep original row order within every digit. The first
 // pass must be stable: later bits alone cannot repair scrambled ties.
 for(uint d=0;d<16;d++){
  uint present=uint(live&&digit==d);
  uint prefix=simd_prefix_exclusive_sum(present),total=simd_sum(present);
  if(digit==d)local=prefix;
  if(slane==0)totals[d*256+sid]=total;
 }
 threadgroup_barrier(mem_flags::mem_threadgroup);
 if(live){for(uint s=0;s<sid;s++)local+=totals[digit*256+s];qlinks[lane]=local;}
 if(tid<16){uint total=0;for(uint s=0;s<groups;s++)total+=totals[tid*256+s];qheads[group*16+tid]=total;}
}
kernel void query_radix_offsets(KERNEL_ARGS,constant uint&bit[[buffer(13)]]){
 if(m.error||!control[0]||lane>=16)return;
 uint groups=(control[7]+255)/256,n=0;
 for(uint g=0;g<groups;g++){uint at=g*16+lane,v=qheads[at];qheads[at]=n;n+=v;}
 qheads[groups*16+lane]=n;
 if(lane==0&&bit+1==QUERY_RADIX_PASSES){control[8]=(2*QBUCKETS+255)/256;control[9]=1;control[10]=1;}
}
kernel void query_radix_scatter(KERNEL_ARGS,constant uint&bit[[buffer(13)]],device ulong*alternate[[buffer(14)]]){
 if(m.error||!control[0]||lane>=control[7])return;
 device ulong*input=(bit&1)?alternate:(device ulong*)matches;device ulong*output=(bit&1)?(device ulong*)matches:alternate;
 ulong key=input[lane];uint digit=uint((key>>(32+4*bit))&15),groups=(control[7]+255)/256;
 uint target=qheads[(lane/256)*16+digit]+qlinks[lane];
 for(uint d=0;d<digit;d++)target+=qheads[groups*16+d];
 output[target]=key;
}
#else
kernel void query_radix_scan(KERNEL_ARGS,constant uint&bit[[buffer(13)]],device ulong*alternate[[buffer(14)]],uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 if(m.error||!control[0])return;threadgroup uint sums[256];
 device ulong*input=(bit&1)?alternate:(device ulong*)matches;
 uint zero=lane<control[7]?uint(((input[lane]>>(32+bit))&1)==0):0;
 sums[tid]=zero;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sums[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sums[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<control[7])qlinks[lane]=sums[tid]-zero;
 if(tid==255)qheads[group]=sums[tid];
}
kernel void query_radix_offsets(KERNEL_ARGS,constant uint&bit[[buffer(13)]]){
 if(lane||m.error||!control[0])return;uint n=0;
 for(uint b=0;b<(control[7]+255)/256;b++){uint v=qheads[b];qheads[b]=n;n+=v;}
 control[2]=n;
 if(bit+1==QUERY_RADIX_BITS){control[8]=(2*QBUCKETS+255)/256;control[9]=1;control[10]=1;}
}
kernel void query_radix_scatter(KERNEL_ARGS,constant uint&bit[[buffer(13)]],device ulong*alternate[[buffer(14)]]){
 if(m.error||!control[0]||lane>=control[7])return;
 device ulong*input=(bit&1)?alternate:(device ulong*)matches;device ulong*output=(bit&1)?(device ulong*)matches:alternate;
 ulong key=input[lane];uint before=qheads[lane/256]+qlinks[lane];
 uint target=((key>>(32+bit))&1)?control[2]+lane-before:before;output[target]=key;
}
#endif
kernel void query_link_sorted(KERNEL_ARGS,device ulong*alternate[[buffer(14)]]){
 if(m.error||!control[0]||lane>=control[7])return;
 device ulong*keys=(QUERY_RADIX_PASSES&1)?alternate:(device ulong*)matches;
 ulong key=keys[lane];uint h=uint(key>>32),e=uint(key);
 if(lane==0||uint(keys[lane-1]>>32)!=h)qheads[h]=e;
 bool last=lane+1==control[7]||uint(keys[lane+1]>>32)!=h;
 if(last)qheads[QBUCKETS+h]=e;
 qlinks[e]=last?0xffffffff:uint(keys[lane+1]);
 set_query_previous(old,qprev,e,(lane==0||uint(keys[lane-1]>>32)!=h)?0xffffffff:uint(keys[lane-1]));
}

long index_canon(device long*r,device uint*p,uint t,uint c,long x){return EQ[t*COLS+c]==1?long(rb_root(p,uint(x))):(EQ[t*COLS+c]==2?rb_set(r,x):x);}
uint index_current_hash(device long*r,device uint*p,uint i){
 uint t=uint(r[i*STRIDE]);ulong h=ulong(t)+0x9e3779b97f4a7c15UL;
 for(uint c=0;c<WIDTH[t]-1;c++){ulong v=ulong(index_canon(r,p,t,c,r[i*STRIDE+4+c]));v^=v>>30;v*=0xbf58476d1ce4e5b9UL;h^=v+(h<<6)+(h>>2);}
 return uint(h^(h>>32))&(HASH_CAP-1);
}
bool index_current_equal(device long*r,device uint*p,uint i,uint j){
 uint t=uint(r[i*STRIDE]);if(r[j*STRIDE]!=t)return false;
 for(uint c=0;c<WIDTH[t]-1;c++)if(index_canon(r,p,t,c,r[i*STRIDE+4+c])!=index_canon(r,p,t,c,r[j*STRIDE+4+c]))return false;
 return true;
}
kernel void row_index_context(KERNEL_ARGS){if(lane||m.error)return;for(long h=2;h<r[ROW_WORDS];h+=r[ROW_WORDS+h]+4)set_canon(r,p,h);}
kernel void row_index_clear(KERNEL_ARGS){if(!m.error&&lane<HASH_CAP)r[INDEX_OFFSET+1+lane]=0;}
kernel void row_index_parallel(KERNEL_ARGS){
 if(m.error||lane>=m.count||r[lane*STRIDE+1]==2)return;
 // Only a suspended rebuild callback uses this restricted index. Its complete
 // transitive constructor vocabulary is known from the typed merge program.
 // Final compaction invalidates it before any ordinary query/action can read it.
 if(MERGE_TARGET_INDEX&&control[0]==2&&!MERGE_TARGET[uint(r[lane*STRIDE])])return;
 device atomic_uint*index=(device atomic_uint*)(r+INDEX_OFFSET+1);uint h=index_current_hash(r,p,lane);
 while(true){
  uint old=atomic_load_explicit(index+2*h,memory_order_relaxed);
  if(!old){uint expected=0;if(atomic_compare_exchange_weak_explicit(index+2*h,&expected,lane+1,memory_order_relaxed,memory_order_relaxed))return;}
  else if(index_current_equal(r,p,lane,old-1)){atomic_fetch_min_explicit(index+2*h,lane+1,memory_order_relaxed);return;}
  else h=(h+1)&(HASH_CAP-1);
 }
}
kernel void row_index_ready(KERNEL_ARGS){if(!lane&&!m.error){r[INDEX_OFFSET]=long(m.unions)+1;r[INDEX_MERGE_VERSION]=p[MERGE_GENERATION];r[INDEX_OFFSET+HASH_CAP+1]++;}}

kernel void query_table_time(KERNEL_ARGS){
 if(m.error||!control[0]||lane>=m.count||old[lane*STRIDE+1]!=0)return;
 uint t=uint(old[lane*STRIDE]);atomic_fetch_max_explicit((device atomic_uint*)&qheads[2*QBUCKETS+1+t],uint(old[lane*STRIDE+2]),memory_order_relaxed);
}
