uint rb_root(device uint*p,uint x){while(p[x]!=x)x=p[x];return x;}
long rb_set(device long*r,long h){
 device long*s=r+ROW_WORDS;
 for(long q=2;q<h;q+=s[q]+4){
  if(s[q+2]!=s[h+2]||s[q+1]!=s[h+1])continue;
  bool same=true;for(uint i=0;i<uint(s[h+1]);i++)same&=s[q+4+i]==s[h+4+i];if(same)return q;
 }
 return h;
}
uint rb_hash(device long*r,uint i){
 uint t=uint(r[i*STRIDE]);ulong h=ulong(t)+0x9e3779b97f4a7c15UL;
 for(uint c=0;c<WIDTH[t]-1;c++){ulong v=ulong(r[i*STRIDE+4+c]);v^=v>>30;v*=0xbf58476d1ce4e5b9UL;h^=v+(h<<6)+(h>>2);}
 return uint(h^(h>>32))&(HASH_CAP-1);
}
bool rb_equal(device long*r,uint a,uint b){
 uint t=uint(r[a*STRIDE]);if(r[b*STRIDE]!=t)return false;
 for(uint c=0;c<WIDTH[t]-1;c++)if(r[a*STRIDE+4+c]!=r[b*STRIDE+4+c])return false;
 return true;
}
kernel void rebuild_start(KERNEL_ARGS){
 if(lane||m.error)return;control[1]=m.count;control[2]=m.unions;control[4]=0;
 for(long h=2;h<r[ROW_WORDS];h+=r[ROW_WORDS+h]+4)set_canon(r,p,h);
}
kernel void rebuild_canonical(KERNEL_ARGS){
 if(m.error)return;
 if(lane<HASH_CAP)qheads[lane]=0;
 if(lane>=control[1]||r[lane*STRIDE+1]==2)return;uint t=uint(r[lane*STRIDE]);
 for(uint c=0;c<WIDTH[t];c++){
  uint at=lane*STRIDE+4+c,kind=EQ[t*COLS+c];long oldv=r[at];
  long v=kind==1?long(rb_root(p,uint(oldv))):(kind==2?rb_set(r,oldv):oldv);
  if(v!=oldv||(kind==2&&r[ROW_WORDS+v+3]==m.epoch)){
   r[at]=v;r[lane*STRIDE+2]=m.epoch;
   atomic_store_explicit((device atomic_uint*)&m.changed,1u,memory_order_relaxed);
  }
 }
}
kernel void rebuild_insert(KERNEL_ARGS){
 if(m.error||lane>=control[1]||r[lane*STRIDE+1]==2)return;
 device atomic_uint*heads=(device atomic_uint*)qheads;uint h=rb_hash(r,lane);
 while(true){
  uint present=atomic_load_explicit(heads+h,memory_order_relaxed);
  if(!present){uint expected=0;if(atomic_compare_exchange_weak_explicit(heads+h,&expected,lane+1,memory_order_relaxed,memory_order_relaxed))return;}
  else if(rb_equal(r,lane,present-1)){atomic_fetch_min_explicit(heads+h,lane+1,memory_order_relaxed);return;}
  else h=(h+1)&(HASH_CAP-1);
 }
}
kernel void rebuild_assign(KERNEL_ARGS){
 if(m.error||lane>=control[1]||r[lane*STRIDE+1]==2)return;
 uint h=rb_hash(r,lane);
 while(qheads[h]&&!rb_equal(r,lane,qheads[h]-1))h=(h+1)&(HASH_CAP-1);
 if(!qheads[h]){atomic_store_explicit((device atomic_uint*)&m.error,12u,memory_order_relaxed);return;}
 qlinks[lane]=qheads[h]-1;
}
kernel void merge_scan(KERNEL_ARGS,uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 if(m.error)return;threadgroup uint offsets[256];
 uint duplicate=(lane<control[1]&&r[lane*STRIDE+1]!=2&&qlinks[lane]!=lane)?1:0;
 offsets[tid]=duplicate;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint stride=1;stride<256;stride*=2){uint previous=tid>=stride?offsets[tid-stride]:0;threadgroup_barrier(mem_flags::mem_threadgroup);offsets[tid]+=previous;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<CAPACITY)qlinks[CAPACITY+lane]=duplicate?offsets[tid]-1:0xffffffff;
 if(tid==255)qheads[group]=offsets[tid];
}
kernel void merge_offsets(KERNEL_ARGS){
 if(lane||m.error)return;uint count=0;
 for(uint b=0;b<(CAPACITY+255)/256;b++){uint n=qheads[b];qheads[b]=count;count+=n;}
 control[7]=count;
}
kernel void merge_scatter(KERNEL_ARGS){
 if(m.error||lane>=control[1]||qlinks[CAPACITY+lane]==0xffffffff)return;
 qlinks[2*CAPACITY+qheads[lane/256]+qlinks[CAPACITY+lane]]=lane;
}
kernel void rebuild_merge(KERNEL_ARGS){
 if(lane)return;if(m.error){control[0]=0;return;}
 for(uint n=control[4];n<control[7]&&!m.error;n++){uint j=qlinks[2*CAPACITY+n];
  uint i=qlinks[j],t=uint(r[i*STRIDE]),oi=i*STRIDE+4+WIDTH[t]-1,oj=j*STRIDE+4+WIDTH[t]-1;
  long a=canon(r,p,t,WIDTH[t]-1,r[oi]),b=canon(r,p,t,WIDTH[t]-1,r[oj]);
  if(PARALLEL_MERGE_INDEX&&a!=b&&MERGE_INDEX[t]&&r[INDEX_OFFSET]!=long(m.unions)+1){control[4]=n;control[0]=2;return;}
  if(POLICY[t]==4&&a!=b){m.error=7;break;}
  r[oi]=mergevalue(t,a,b,r,p,fp,m);
  r[i*STRIDE+1]=max(r[i*STRIDE+1],r[j*STRIDE+1]);r[i*STRIDE+2]=m.epoch;r[j*STRIDE+1]=2;
  fp[root(fp,uint(r[j*STRIDE+3]))]=root(fp,uint(r[i*STRIDE+3]));m.changed=1;
  // Group representatives and the index both choose the earliest live row.
  // Retiring j cannot retire that representative. New unions still invalidate
  // the index through its union-count version; compaction invalidates it later.
 }
 control[0]=!m.error&&(control[2]!=m.unions||control[1]!=m.count);
 control[3]=m.error;
}
kernel void compact_scan(KERNEL_ARGS,uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 if(m.error)return;
 threadgroup uint offsets[256];uint live=(lane<m.count&&r[lane*STRIDE+1]!=2)?1:0;
 offsets[tid]=live;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint stride=1;stride<256;stride*=2){
  uint previous=tid>=stride?offsets[tid-stride]:0;
  threadgroup_barrier(mem_flags::mem_threadgroup);offsets[tid]+=previous;threadgroup_barrier(mem_flags::mem_threadgroup);
 }
 if(lane<CAPACITY)qlinks[lane]=live?offsets[tid]-1:0xffffffff;
 if(tid==255)qheads[group]=offsets[tid];
}
kernel void compact_offsets(KERNEL_ARGS){
 if(lane||m.error)return;uint count=0;
 for(uint b=0;b<(CAPACITY+255)/256;b++){uint n=qheads[b];qheads[b]=count;count+=n;}
 control[1]=count;control[2]=m.count;
}
kernel void compact_copy(KERNEL_ARGS){
 if(m.error||control[1]==control[2]||lane>=control[2]||qlinks[lane]==0xffffffff)return;
 uint to=qheads[lane/256]+qlinks[lane];
 for(uint c=0;c<STRIDE;c++)matches[to*STRIDE+c]=r[lane*STRIDE+c];
}
kernel void compact_store(KERNEL_ARGS){
 if(m.error)return;
 if(control[1]!=control[2]&&lane<control[1]*STRIDE)r[lane]=matches[lane];
 if(lane==0){m.count=control[1];r[INDEX_OFFSET]=0;r[QUERY_DIRTY]=1;}
}
