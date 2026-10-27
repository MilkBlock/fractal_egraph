// An immutable view is reconstructed by base copy + exact word updates.
// Integer words also preserve the exact bits of i64/f64 fields.
uint sc_position(uint i,device const uint*domain,uint4 cfg){
 if(!cfg.z)return i;uint indexed=cfg.w*STRIDE*2;
 return i<indexed?domain[i/(STRIDE*2)]*STRIDE*2+i%(STRIDE*2):cfg.x+(i-indexed);
}
kernel void sc_count(device const uint*before[[buffer(0)]],device const uint*after[[buffer(1)]],device atomic_uint*count[[buffer(2)]],device uint*blocks[[buffer(3)]],device const uint*domain[[buffer(4)]],constant uint4&length[[buffer(5)]],uint i[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]],uint lane[[thread_index_in_simdgroup]],uint sg[[simdgroup_index_in_threadgroup]],uint width[[threads_per_simdgroup]]){
 uint work=length.z?length.w*STRIDE*2+(length.y>length.x?length.y-length.x:0):length.y;uint at=i<work?sc_position(i,domain,length):length.y;
 threadgroup uint sums[256];uint changed=at<length.y&&(at>=length.x||before[at]!=after[at]),total=simd_sum(changed);
 if(lane==0)sums[sg]=total;threadgroup_barrier(mem_flags::mem_threadgroup);
 if(tid==0){uint n=0;for(uint j=0;j<(256+width-1)/width;j++)n+=sums[j];blocks[group]=n;if(n)atomic_fetch_add_explicit(count,n,memory_order_relaxed);}
}
kernel void sc_pack(device const uint*before[[buffer(0)]],device const uint*after[[buffer(1)]],device uint2*patch[[buffer(2)]],device atomic_uint*cursor[[buffer(3)]],device const uint*blocks[[buffer(4)]],device const uint*domain[[buffer(5)]],constant uint4&length[[buffer(6)]],uint i[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 if(!blocks[group])return;
 uint work=length.z?length.w*STRIDE*2+(length.y>length.x?length.y-length.x:0):length.y;uint at=i<work?sc_position(i,domain,length):length.y;
 threadgroup uint sums[256];threadgroup uint base;uint changed=at<length.y&&(at>=length.x||before[at]!=after[at]);sums[tid]=changed;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sums[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sums[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(tid==0)base=sums[255]?atomic_fetch_add_explicit(cursor,sums[255],memory_order_relaxed):0;threadgroup_barrier(mem_flags::mem_threadgroup);
 if(changed)patch[base+sums[tid]-1]=uint2(at,after[at]);
}
kernel void sc_apply(device const uint*header[[buffer(0)]],device const uint2*patch[[buffer(1)]],device uint*out[[buffer(2)]],device atomic_uint*error[[buffer(3)]],constant uint4&request[[buffer(4)]],uint i[[thread_position_in_grid]]){
 // request = expected base version, expected target version, extent, count.
 if(header[0]!=request.x||header[1]!=request.y){if(i==0)atomic_store_explicit(error,1u,memory_order_relaxed);return;}
 if(i<request.w){uint2 item=patch[i];if(item.x>=request.z){atomic_store_explicit(error,2u,memory_order_relaxed);return;}out[item.x]=item.y;}
}
kernel void sc_equal(device const uint*a[[buffer(0)]],device const uint*b[[buffer(1)]],device atomic_uint*error[[buffer(2)]],constant uint&n[[buffer(3)]],uint i[[thread_position_in_grid]]){
 if(i<n&&a[i]!=b[i])atomic_store_explicit(error,3u,memory_order_relaxed);
}
uint sc_root(device const uint*p,uint x){while(p[x]!=x)x=p[x];return x;}
// One dispatch scans independently frozen versions. A threadgroup has one
// context, so it never mixes rows from different versions in one binding.
kernel void sc_scan(device const long*r0[[buffer(0)]],device const long*r1[[buffer(1)]],device const long*r2[[buffer(2)]],device const long*r3[[buffer(3)]],device const uint*p0[[buffer(4)]],device const uint*p1[[buffer(5)]],device const uint*p2[[buffer(6)]],device const uint*p3[[buffer(7)]],device const uint*headers[[buffer(8)]],device atomic_uint*out[[buffer(9)]],constant uint4&cfg[[buffer(10)]],device const uint*readMask[[buffer(11)]],uint2 pos[[thread_position_in_grid]]){
 uint bank=pos.y,i=pos.x;if(bank>=cfg.x)return;device const uint*h=headers+bank*8;
 if(h[0]!=h[1]){if(i==0)atomic_store_explicit(out+bank*(cfg.y+2),1u,memory_order_relaxed);return;}
 if(i>=h[2])return;device const long*r=bank==0?r0:bank==1?r1:bank==2?r2:r3;device const uint*p=bank==0?p0:bank==1?p1:bank==2?p2:p3;
 uint t=uint(r[i*STRIDE]);if(t>=cfg.y||!readMask[bank*cfg.y+t]||r[i*STRIDE+1])return;
 atomic_fetch_add_explicit(out+bank*(cfg.y+2)+2+t,1u,memory_order_relaxed);
 for(uint j=0;j<WIDTH[t];j++)if(EQ[t*COLS+j]==1){uint id=uint(r[i*STRIDE+4+j]);if(id>=h[3]){atomic_store_explicit(out+bank*(cfg.y+2),2u,memory_order_relaxed);return;}if(sc_root(p,id)!=id)atomic_fetch_add_explicit(out+bank*(cfg.y+2)+1,1u,memory_order_relaxed);}
}
kernel void sc_probe(device const long*r0[[buffer(0)]],device const long*r1[[buffer(1)]],device const long*r2[[buffer(2)]],device const long*r3[[buffer(3)]],device const uint*p0[[buffer(4)]],device const uint*p1[[buffer(5)]],device const uint*p2[[buffer(6)]],device const uint*p3[[buffer(7)]],device const uint*headers[[buffer(8)]],device long*out[[buffer(9)]],device const uint*queries[[buffer(10)]],constant uint&count[[buffer(11)]],uint i[[thread_position_in_grid]]){
 if(i>=count)return;device const uint*q=queries+6*i;device long*answer=out+4*i;for(uint j=0;j<4;j++)answer[j]=0;
 uint bank=q[0];if(bank>=4||headers[bank*8]!=q[1]){answer[0]=1;return;}device const uint*h=headers+bank*8;
 device const long*r=bank==0?r0:bank==1?r1:bank==2?r2:r3;device const uint*p=bank==0?p0:bank==1?p1:bank==2?p2:p3;
 if(q[2]==0){if(q[3]>=h[3]||q[4]>=h[3]){answer[0]=2;return;}uint a=sc_root(p,q[3]),b=sc_root(p,q[4]);answer[1]=a==b;answer[2]=a;answer[3]=b;}
 else if(q[2]==1){uint table=q[3];if(table>=q[5]||WIDTH[table]!=2||q[4]>=h[3]){answer[0]=2;return;}
  for(uint row=0;row<h[2];row++)if(uint(r[row*STRIDE])==table&&!r[row*STRIDE+1]&&sc_root(p,uint(r[row*STRIDE+4]))==sc_root(p,q[4])){
   answer[1]=1;answer[2]=EQ[table*COLS+1]==1?long(sc_root(p,uint(r[row*STRIDE+5]))):r[row*STRIDE+5];answer[3]=row;return;}}
 else answer[0]=3;
}
// The certified scope cannot change container contents. Its arena epoch is a
// cache tag, not membership; per-view visibility uses the frozen Meta epoch.
kernel void sc_arena_audit(device const long*before[[buffer(0)]],device const long*world[[buffer(1)]],device atomic_uint*error[[buffer(2)]],constant uint&n[[buffer(3)]],uint i[[thread_position_in_grid]]){
 if(i<n&&i!=1&&before[i]!=world[ROW_WORDS+i])atomic_store_explicit(error,4u,memory_order_relaxed);
}
