// A segment contains either independent constructor recipes or the original
// ordered actions. Nothing observes incomplete proposals. uint index writes use
// the low half of the existing zero-high-half 64-bit row-index entries.
#define BATCH_ARGS KERNEL_ARGS,constant uint&segment[[buffer(27)]],device uint*bs[[buffer(28)]],device long*proposals[[buffer(29)]]
// BATCH_RECIPES
uint batch_binding(uint ordinal,device uint*bs){return bs[BATCH_ACTIVE+ordinal];}
uint batch_lower(uint rank,device long*matches,device uint*bs){
 uint lo=0,hi=bs[35];while(lo<hi){uint mid=(lo+hi)/2,b=batch_binding(mid,bs);if(uint(matches[b*MSTRIDE+1+VARS])<rank)lo=mid+1;else hi=mid;}return lo;
}
uint batch_hash(device long*a){uint t=uint(a[0]);ulong h=ulong(t)+0x9e3779b97f4a7c15UL;for(uint c=0;c<WIDTH[t]-1;c++){ulong v=ulong(a[c+2]);v^=v>>30;v*=0xbf58476d1ce4e5b9UL;h^=v+(h<<6)+(h>>2);}return uint(h^(h>>32));}
bool batch_equal(device long*a,device long*b){uint t=uint(a[0]);if(b[0]!=t)return false;for(uint c=0;c<WIDTH[t]-1;c++)if(a[c+2]!=b[c+2])return false;return true;}
kernel void batch_begin(BATCH_ARGS){if(lane)return;for(uint i=0;i<64;i++)bs[i]=0;for(uint j=0;j<4;j++){bs[17+j*3]=bs[18+j*3]=1;}bs[41]=bs[42]=1;bs[8]=m.unions;bs[31]=0;bs[28]=m.error?0:1;bs[29]=1;bs[30]=1;p[IDCAP]=DEFER_UNIONS[set];p[IDCAP+1]=0;}
kernel void batch_active_scan(BATCH_ARGS,uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 if(m.error)return;threadgroup uint sums[256];uint live=0;
 if(lane<m.matched){uint b=m.matched>1?match_order[lane]:lane;live=!((NOOP_PREFLIGHT||EFFECT_DEDUP)&&noop[b]);}
 sums[tid]=live;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sums[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sums[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<m.matched)bs[BATCH_PREFIX+lane]=sums[tid]-live;if(tid==255)bs[BATCH_BLOCK+group]=sums[tid];
}
kernel void batch_active_offsets(BATCH_ARGS){
 if(lane)return;uint n=0;if(!m.error)for(uint b=0;b<(m.matched+255)/256;b++){uint v=bs[BATCH_BLOCK+b];bs[BATCH_BLOCK+b]=n;n+=v;}bs[35]=n;
}
kernel void batch_active_scatter(BATCH_ARGS){
 if(m.error||lane>=m.matched)return;uint b=m.matched>1?match_order[lane]:lane;if((NOOP_PREFLIGHT||EFFECT_DEDUP)&&noop[b])return;
 bs[BATCH_ACTIVE+bs[BATCH_BLOCK+lane/256]+bs[BATCH_PREFIX+lane]]=b;
}
kernel void batch_open(BATCH_ARGS){
 if(lane)return;for(uint j=0;j<4;j++){bs[16+j*3]=0;bs[17+j*3]=1;bs[18+j*3]=1;}bs[0]=0;bs[1]=0;bs[4]=0;bs[37]=0;bs[40]=0;bs[41]=bs[42]=1;
 if(m.error){bs[28]=0;return;}
 // Only a control cursor is serial. Unsupported actions retain exact order;
 // no-op bindings and empty rule segments have already disappeared on GPU.
 while(bs[31]<bs[35]){
  uint start=bs[31],binding=batch_binding(start,bs),rule=uint(matches[binding*MSTRIDE]);
  if(!BATCH_RULE[rule]){execute_actions(rule,binding,code,registers,matches,r,p,fp,m);bs[31]++;if(m.error){bs[28]=0;return;}continue;}
  uint rank=uint(matches[binding*MSTRIDE+1+VARS]),end=batch_lower(BATCH_RANK_END[SET_START[set]+rank],matches,bs);
  if(end-start<256){
   for(uint i=start;i<end;i++){uint b=batch_binding(i,bs);execute_actions(uint(matches[b*MSTRIDE]),b,code,registers,matches,r,p,fp,m);if(m.error){bs[28]=0;return;}}
   bs[31]=end;continue;
  }
  ensure_row_index(r,p,m);bs[31]=end;bs[1]=end-start;bs[37]=end-start;bs[2]=start;
  uint sizes[4]={0,(end-start+255)/256,1,1};for(uint j=0;j<4;j++)bs[16+j*3]=sizes[j];return;
 }
 bs[28]=0;
}
kernel void batch_output_scan(BATCH_ARGS,uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 if(m.error||bs[0])return;threadgroup uint sums[256];uint count=0;
 if(lane<bs[37]){uint binding=batch_binding(bs[2]+lane,bs);count=BATCH_WRITES[uint(matches[binding*MSTRIDE])];}
 sums[tid]=count;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sums[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sums[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<bs[37])bs[BATCH_EFFECT_OFFSET+lane]=sums[tid]-count;if(tid==255)bs[BATCH_BLOCK+group]=sums[tid];
}
kernel void batch_output_offsets(BATCH_ARGS){
 if(lane||m.error)return;uint n=0;for(uint b=0;b<(bs[37]+255)/256;b++){uint v=bs[BATCH_BLOCK+b];bs[BATCH_BLOCK+b]=n;n+=v;}
 // Workspace exhaustion selects exact ordered execution; no output is dropped.
 if(n>MATCHCAP){bs[0]=1;bs[16]=bs[19]=bs[22]=bs[40]=0;return;}
 bs[1]=n;uint size=2;while(size<2*n)size*=2;bs[3]=size;bs[16]=(size+255)/256;bs[19]=(n+255)/256;bs[40]=(bs[37]+255)/256;bs[41]=bs[42]=1;
}
kernel void batch_clear(BATCH_ARGS){if(lane<bs[3])bs[BATCH_HASH+lane]=0;}
kernel void batch_propose(BATCH_ARGS){
 if(m.error||lane>=bs[37])return;uint binding=batch_binding(bs[2]+lane,bs),rule=uint(matches[binding*MSTRIDE]);
 uint base=bs[BATCH_BLOCK+lane/256]+bs[BATCH_EFFECT_OFFSET+lane],count=BATCH_WRITES[rule];device long*out=proposals+base*BATCH_STRIDE;
 for(uint j=0;j<count;j++){out[j*BATCH_STRIDE]=-1;out[j*BATCH_STRIDE+1]=-1;}
 if(!batch_recipe(rule,binding,matches,out)){atomic_store_explicit((device atomic_uint*)bs,1u,memory_order_relaxed);return;}
 for(uint j=0;j<count;j++){
  device long*entry=out+j*BATCH_STRIDE;uint t=uint(entry[0]);long key[COLS]={0};for(uint c=0;c<WIDTH[t]-1;c++)key[c]=entry[c+2];
  long found=query_lookup(r,t,key);entry[1]=found;
  if(found>=long(r[QUERY_LOG+1])&&r[CONSTRUCTOR_PRED+found]==0)atomic_store_explicit((device atomic_uint*)bs,1u,memory_order_relaxed);
 }
 atomic_fetch_add_explicit((device atomic_uint*)(bs+9),count,memory_order_relaxed);
}
kernel void batch_group(BATCH_ARGS){
 if(m.error||bs[0]||lane>=bs[1])return;device long*a=proposals+lane*BATCH_STRIDE;if(a[0]<0||a[1]>=0)return;
 device atomic_uint*heads=(device atomic_uint*)(bs+BATCH_HASH);uint mask=bs[3]-1,h=batch_hash(a)&mask;
 while(true){uint old=atomic_load_explicit(heads+h,memory_order_relaxed);if(!old){uint expected=0;if(atomic_compare_exchange_weak_explicit(heads+h,&expected,lane+1,memory_order_relaxed,memory_order_relaxed))return;}
 else if(batch_equal(a,proposals+(old-1)*BATCH_STRIDE)){atomic_fetch_min_explicit(heads+h,lane+1,memory_order_relaxed);return;}else h=(h+1)&mask;}
}
kernel void batch_scan(BATCH_ARGS,uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 if(m.error||bs[0])return;threadgroup uint sums[256];uint leader=0xffffffff;
 if(lane<bs[1]){device long*a=proposals+lane*BATCH_STRIDE;if(a[0]>=0&&a[1]<0){uint mask=bs[3]-1,h=batch_hash(a)&mask;while(bs[BATCH_HASH+h]&&!batch_equal(a,proposals+(bs[BATCH_HASH+h]-1)*BATCH_STRIDE))h=(h+1)&mask;leader=bs[BATCH_HASH+h]-1;}}
 uint live=leader==lane;sums[tid]=live;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sums[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sums[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<bs[1]){bs[BATCH_LEADER+lane]=leader;bs[BATCH_PREFIX+lane]=sums[tid]-live;}if(tid==255)bs[BATCH_BLOCK+group]=sums[tid];
}
kernel void batch_reserve(BATCH_ARGS){
 if(lane||m.error||bs[0])return;uint n=0;for(uint b=0;b<(bs[1]+255)/256;b++){uint v=bs[BATCH_BLOCK+b];bs[BATCH_BLOCK+b]=n;n+=v;}
 if(ulong(m.count)+n>CAPACITY||ulong(m.next_id)+n>IDCAP||ulong(m.next_fact)+n>IDCAP){bs[0]=1;return;}
 bs[4]=n;bs[5]=m.count;bs[6]=m.next_id;bs[7]=m.next_fact;m.count+=n;m.next_id+=n;m.next_fact+=n;if(n)m.changed=1;
 bs[10]+=n;bs[12]++;
}
kernel void batch_publish(BATCH_ARGS){
 if(m.error||bs[0]||lane>=bs[1]||bs[BATCH_LEADER+lane]!=lane)return;
 uint ordinal=bs[BATCH_BLOCK+lane/256]+bs[BATCH_PREFIX+lane],i=bs[5]+ordinal,value=bs[6]+ordinal,fid=bs[7]+ordinal;
 device long*a=proposals+lane*BATCH_STRIDE;uint t=uint(a[0]);p[value]=value;p[MERGE_KEY_FLAGS+value]=MERGE_KEY_OUTPUT[t];fp[fid]=fid;
 r[i*STRIDE]=t;r[i*STRIDE+1]=0;r[i*STRIDE+2]=m.epoch;r[i*STRIDE+3]=fid;
 for(uint c=0;c<WIDTH[t]-1;c++)r[i*STRIDE+4+c]=a[c+2];r[i*STRIDE+4+WIDTH[t]-1]=value;r[CONSTRUCTOR_PRED+i]=value;
}
kernel void batch_index(BATCH_ARGS){
 if(m.error||bs[0]||lane>=bs[1]||bs[BATCH_LEADER+lane]!=lane)return;
 uint ordinal=bs[BATCH_BLOCK+lane/256]+bs[BATCH_PREFIX+lane],i=bs[5]+ordinal;device long*a=proposals+lane*BATCH_STRIDE;
 uint h=batch_hash(a)&(HASH_CAP-1);device atomic_uint*index=(device atomic_uint*)(r+INDEX_OFFSET+1);
 while(true){uint old=atomic_load_explicit(index+2*h,memory_order_relaxed);if(!old){uint expected=0;if(atomic_compare_exchange_weak_explicit(index+2*h,&expected,i+1,memory_order_relaxed,memory_order_relaxed))return;}else h=(h+1)&(HASH_CAP-1);}
}
kernel void batch_ordered(BATCH_ARGS){
 if(lane||m.error||!bs[0])return;bs[11]++;
 for(uint i=0;i<bs[37];i++){uint binding=batch_binding(bs[2]+i,bs);if((NOOP_PREFLIGHT||EFFECT_DEDUP)&&noop[binding])continue;
 execute_actions(uint(matches[binding*MSTRIDE]),binding,code,registers,matches,r,p,fp,m);if(m.error)return;}
}
kernel void batch_finish(BATCH_ARGS){
 if(lane||m.error)return;p[IDCAP]=0;for(uint i=0;i<p[IDCAP+1];i++)unite_now(p,p[IDCAP+2+2*i],p[IDCAP+3+2*i],m);
 control[0]=0;control[3]=m.error;if(m.unions!=bs[8]){if(PARALLEL_REBUILD)control[0]=1;else rebuild(r,p,fp,m,qheads,qlinks);}
 for(uint i=SET_START[set];i<SET_START[set+1];i++)watermarks[SET_RULES[i]]=m.epoch-1;
}
