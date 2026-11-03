bool match_before(uint a,uint b,device long*matches){
 if(a==0xffffffff)return false;if(b==0xffffffff)return true;
 for(uint c=1+VARS;c<MSTRIDE;c++){long x=matches[a*MSTRIDE+c],y=matches[b*MSTRIDE+c];if(x!=y)return x<y;}
 return a<b;
}
kernel void match_order_local(KERNEL_ARGS,uint tid[[thread_index_in_threadgroup]]){
 threadgroup uint ids[256];ids[tid]=lane<m.matched?lane:0xffffffff;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint k=2;k<=256;k*=2)for(uint d=k/2;d;d/=2){
  uint other=tid^d,a=ids[tid],b=ids[other];bool low=((lane&k)==0)==(tid<other),before=match_before(a,b,matches);
  threadgroup_barrier(mem_flags::mem_threadgroup);ids[tid]=low?(before?a:b):(before?b:a);threadgroup_barrier(mem_flags::mem_threadgroup);
 }
 match_order[lane]=ids[tid];
}
kernel void match_order_global(KERNEL_ARGS,constant uint&k[[buffer(16)]],constant uint&d[[buffer(17)]]){
 uint other=lane^d;if(other<=lane)return;
 uint a=match_order[lane],b=match_order[other];bool ascending=(lane&k)==0;
 if(match_before(a,b,matches)!=ascending){match_order[lane]=b;match_order[other]=a;}
}
kernel void match_order_merge(KERNEL_ARGS,constant uint&k[[buffer(16)]],uint tid[[thread_index_in_threadgroup]]){
 threadgroup uint ids[256];ids[tid]=match_order[lane];threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=128;d;d/=2){uint other=tid^d,a=ids[tid],b=ids[other];bool low=((lane&k)==0)==(tid<other),before=match_before(a,b,matches);
  threadgroup_barrier(mem_flags::mem_threadgroup);ids[tid]=low?(before?a:b):(before?b:a);threadgroup_barrier(mem_flags::mem_threadgroup);
 }
 match_order[lane]=ids[tid];
}
