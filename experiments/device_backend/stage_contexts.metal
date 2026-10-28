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
kernel void sc_scan(device const long*r0[[buffer(0)]],device const long*r1[[buffer(1)]],device const long*r2[[buffer(2)]],device const long*r3[[buffer(3)]],device const uint*p0[[buffer(4)]],device const uint*p1[[buffer(5)]],device const uint*p2[[buffer(6)]],device const uint*p3[[buffer(7)]],device const uint*headers[[buffer(8)]],device atomic_uint*out[[buffer(9)]],constant uint4&cfg[[buffer(10)]],device const uint*readMask[[buffer(11)]],device const uint*map0[[buffer(12)]],device const uint*map1[[buffer(13)]],device const uint*map2[[buffer(14)]],device const uint*map3[[buffer(15)]],uint2 pos[[thread_position_in_grid]]){
 uint bank=pos.y,i=pos.x;if(bank>=cfg.x)return;device const uint*h=headers+bank*8;
 if(h[0]!=h[1]){if(i==0)atomic_store_explicit(out+bank*(cfg.y+2),1u,memory_order_relaxed);return;}
 if(i>=h[2])return;SCRows r{bank==0?r0:bank==1?r1:bank==2?r2:r3,bank==0?map0:bank==1?map1:bank==2?map2:map3};device const uint*p=bank==0?p0:bank==1?p1:bank==2?p2:p3;
 uint t=uint(r[i*STRIDE]);if(t>=cfg.y||!readMask[bank*cfg.y+t]||r[i*STRIDE+1])return;
 atomic_fetch_add_explicit(out+bank*(cfg.y+2)+2+t,1u,memory_order_relaxed);
 for(uint j=0;j<WIDTH[t];j++)if(EQ[t*COLS+j]==1){uint id=uint(r[i*STRIDE+4+j]);if(id>=h[3]){atomic_store_explicit(out+bank*(cfg.y+2),2u,memory_order_relaxed);return;}if(sc_root(p,id)!=id)atomic_fetch_add_explicit(out+bank*(cfg.y+2)+1,1u,memory_order_relaxed);}
}
kernel void sc_probe(device const long*r0[[buffer(0)]],device const long*r1[[buffer(1)]],device const long*r2[[buffer(2)]],device const long*r3[[buffer(3)]],device const uint*p0[[buffer(4)]],device const uint*p1[[buffer(5)]],device const uint*p2[[buffer(6)]],device const uint*p3[[buffer(7)]],device const uint*headers[[buffer(8)]],device long*out[[buffer(9)]],device const uint*queries[[buffer(10)]],constant uint&count[[buffer(11)]],device const uint*map0[[buffer(12)]],device const uint*map1[[buffer(13)]],device const uint*map2[[buffer(14)]],device const uint*map3[[buffer(15)]],uint i[[thread_position_in_grid]]){
 if(i>=count)return;device const uint*q=queries+6*i;device long*answer=out+4*i;for(uint j=0;j<4;j++)answer[j]=0;
 uint bank=q[0];if(bank>=4||headers[bank*8]!=q[1]){answer[0]=1;return;}device const uint*h=headers+bank*8;
 SCRows r{bank==0?r0:bank==1?r1:bank==2?r2:r3,bank==0?map0:bank==1?map1:bank==2?map2:map3};device const uint*p=bank==0?p0:bank==1?p1:bank==2?p2:p3;
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

// Pages are reference counted on the device and allocated on demand.
// Releasing a view returns its pages after prior GPU readers.
kernel void sc_page_init(device uint*free[[buffer(0)]],device atomic_uint*refs[[buffer(1)]],device uint*ctrl[[buffer(2)]],constant uint2&cfg[[buffer(3)]],uint i[[thread_position_in_grid]]){
 if(i)return;uint n=cfg.x?ctrl[0]:0;if(!cfg.x){ctrl[1]=0;ctrl[2]=0;ctrl[3]=0;}
 for(uint p=cfg.x;p<cfg.y;p++){free[n++]=p;atomic_store_explicit(refs+p,0u,memory_order_relaxed);}ctrl[0]=n;
}
// Only pages published by a real write (or an appended row) are inspected.
// The optional audit below still compares every byte, so a missing hook fails.
kernel void sc_page_changes(device const uint*live[[buffer(0)]],device const uint*data[[buffer(1)]],device const uint*map[[buffer(2)]],device uint*blocks[[buffer(3)]],device atomic_uint*count[[buffer(4)]],device atomic_uint*changedPages[[buffer(5)]],constant uint4&cfg[[buffer(6)]],device const Meta&m[[buffer(7)]],device const uint*journal[[buffer(8)]],uint job[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 uint page=journal[1+job],words=m.count*STRIDE*2;
 if(page*256>=words)return;
 if(page*256+256<=cfg.x&&live[(JP_DIRTY+page)*2]<=cfg.z){if(!tid)blocks[page]=0;return;}
 uint at=page*256+tid,v=at<words?live[at]:0,old=at<cfg.x?data[map[1+page]*256+tid]:0;
 threadgroup uint sums[256];sums[tid]=at<words&&(at>=cfg.x||v!=old);threadgroup_barrier(mem_flags::mem_threadgroup);
 if(!tid){uint n=0;for(uint j=0;j<256;j++)n+=sums[j];blocks[page]=n;if(n){atomic_fetch_add_explicit(count,n,memory_order_relaxed);atomic_fetch_add_explicit(changedPages,1u,memory_order_relaxed);}}
}
kernel void sc_page_snapshot(device const uint*live[[buffer(0)]],device uint*data[[buffer(1)]],device const uint*oldmap[[buffer(2)]],device uint*newmap[[buffer(3)]],device uint*free[[buffer(4)]],device atomic_uint*refs[[buffer(5)]],device atomic_uint*ctrl[[buffer(6)]],device const uint*blocks[[buffer(7)]],constant uint2&cfg[[buffer(8)]],device uint*copies[[buffer(9)]],uint page[[thread_position_in_grid]]){
 if(page>=(cfg.y+255)/256)return;bool changed=cfg.x==0||blocks[page]!=0;uint target;
 if(changed){uint avail=atomic_fetch_sub_explicit(ctrl,1u,memory_order_relaxed);if(!avail){atomic_store_explicit(ctrl+1,1u,memory_order_relaxed);target=0xffffffff;}else{target=free[avail-1];atomic_store_explicit(refs+target,1u,memory_order_relaxed);atomic_fetch_add_explicit(ctrl+3,1u,memory_order_relaxed);}
  uint q=atomic_fetch_add_explicit((device atomic_uint*)copies,1u,memory_order_relaxed);copies[1+q]=page;
 }else{target=oldmap[1+page];atomic_fetch_add_explicit(refs+target,1u,memory_order_relaxed);}newmap[1+page]=target;
}
kernel void sc_page_release(device const uint*map[[buffer(0)]],device uint*free[[buffer(1)]],device atomic_uint*refs[[buffer(2)]],device atomic_uint*ctrl[[buffer(3)]],constant uint&n[[buffer(4)]],uint i[[thread_position_in_grid]]){
 if(i>=n)return;uint p=map[1+i];if(p==0xffffffff)return;
 if(atomic_fetch_sub_explicit(refs+p,1u,memory_order_relaxed)==1){uint pos=atomic_fetch_add_explicit(ctrl,1u,memory_order_relaxed);free[pos]=p;}
}
kernel void sc_page_flat(device const uint*data[[buffer(0)]],device const uint*map[[buffer(1)]],device uint*out[[buffer(2)]],constant uint&n[[buffer(3)]],uint i[[thread_position_in_grid]]){if(i<n)out[i]=data[map[1+i/256]*256+i%256];}
kernel void sc_page_audit(device const uint*data[[buffer(0)]],device const uint*map[[buffer(1)]],device const uint*live[[buffer(2)]],device atomic_uint*error[[buffer(3)]],constant uint&n[[buffer(4)]],uint i[[thread_position_in_grid]]){if(i<n&&data[map[1+i/256]*256+i%256]!=live[i])atomic_store_explicit(error,3u,memory_order_relaxed);}

// A small, contiguous column view is patched and materialized in one pass.
// Offset/value evidence is retained only for debug export; counts are exact.
kernel void sc_patch_flat(device const uint*before[[buffer(0)]],device const uint*after[[buffer(1)]],device uint*out[[buffer(2)]],device uint2*delta[[buffer(3)]],device atomic_uint*count[[buffer(4)]],constant uint4&cfg[[buffer(5)]],uint i[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]],uint lane[[thread_index_in_simdgroup]],uint sg[[simdgroup_index_in_threadgroup]],uint width[[threads_per_simdgroup]]){
 threadgroup uint sums[256];threadgroup uint base;uint old=i<cfg.x?before[i]:0,newv=i<cfg.y?after[i]:0;uint changed=i<cfg.y&&(i>=cfg.x||old!=newv);
 if(i<cfg.y)out[i]=changed?newv:old;
 if(!cfg.z){uint subtotal=simd_sum(changed);if(!lane)sums[sg]=subtotal;threadgroup_barrier(mem_flags::mem_threadgroup);if(!tid){uint n=0;for(uint j=0;j<(256+width-1)/width;j++)n+=sums[j];if(n)atomic_fetch_add_explicit(count,n,memory_order_relaxed);}return;}
 sums[tid]=changed;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sums[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sums[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(!tid)base=sums[255]?atomic_fetch_add_explicit(count,sums[255],memory_order_relaxed):0;threadgroup_barrier(mem_flags::mem_threadgroup);
 if(cfg.z&&changed)delta[base+sums[tid]-1]=uint2(i,newv);
}
kernel void sc_page_dispatch(device const Meta&m[[buffer(0)]],device uint*args[[buffer(1)]],uint i[[thread_position_in_grid]]){if(!i){args[0]=((m.count*STRIDE*2+255)/256+255)/256;args[1]=1;args[2]=1;}}

kernel void sc_page_copy(device const uint*live[[buffer(0)]],device uint*data[[buffer(1)]],device const uint*map[[buffer(2)]],device const uint*copies[[buffer(3)]],constant uint&words[[buffer(4)]],uint job[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 uint page=copies[job+1],target=map[page+1],at=page*256+tid;if(target!=0xffffffff)data[target*256+tid]=at<words?live[at]:0;
}
kernel void sc_journal_list(device const uint*live[[buffer(0)]],device const Meta&m[[buffer(1)]],device uint*blocks[[buffer(2)]],device uint*journal[[buffer(3)]],constant uint2&cfg[[buffer(4)]],uint page[[thread_position_in_grid]]){
 uint words=m.count*STRIDE*2;if(page*256>=words)return;blocks[page]=0;
 if(page*256+256>cfg.x||live[(JP_DIRTY+page)*2]>cfg.y){uint at=atomic_fetch_add_explicit((device atomic_uint*)journal,1u,memory_order_relaxed);journal[at+1]=page;}
}
kernel void sc_journal_dispatch(device const uint*journal[[buffer(0)]],device uint*args[[buffer(1)]],uint i[[thread_position_in_grid]]){if(!i){args[0]=journal[0];args[1]=1;args[2]=1;}}
