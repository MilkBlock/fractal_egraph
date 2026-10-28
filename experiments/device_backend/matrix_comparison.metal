// Private equality closure. Global tables/UF are immutable until cu_publish.
// COMPARISON_PROGRAM
// cfg = set, row count, id extent, cost count, choice count, hash size, audit.
#define CU_ARGS device long*r[[buffer(0)]],device uint*c[[buffer(1)]],constant uint*cfg[[buffer(2)]],device const uint*svmap[[buffer(28)]],device const long*svdata[[buffer(30)]]
uint cu_root(device const uint*p,uint x){while(p[x]!=x)x=p[x];return x;}
void cu_union(device uint*p,uint a,uint b,device uint*c,device long*r){
 device atomic_uint*uf=(device atomic_uint*)p;
 while(true){uint x=parallel_root(uf,a),y=parallel_root(uf,b);if(x==y)return;uint hi=max(x,y),lo=min(x,y),expected=hi;
  if(atomic_compare_exchange_weak_explicit(uf+hi,&expected,lo,memory_order_relaxed,memory_order_relaxed)){atomic_fetch_add_explicit((device atomic_uint*)(c+5),1u,memory_order_relaxed);if(jp_active(r)&&!JP_BLOCK_EFFECTS)atomic_fetch_max_explicit((device atomic_uint*)(r+JP_VERSION+lo),uint(r[ROW_WORDS+1])+1,memory_order_relaxed);return;}}
}
kernel void cu_count(CU_ARGS,uint i[[thread_position_in_grid]]){
 if(i>=cfg[1]||sv[i*STRIDE+1]==2)return;uint3 s=cu_schema(cfg[0]);uint t=uint(sv[i*STRIDE]);
 if(t==s.x||t==s.y){if(sv[i*STRIDE+1])atomic_store_explicit((device atomic_uint*)(c+6),1u,memory_order_relaxed);
 atomic_fetch_add_explicit((device atomic_uint*)(c+(t==s.x?0:1)),1u,memory_order_relaxed);}
}
kernel void cu_pack(CU_ARGS,device uint*costs[[buffer(3)]],device uint*choices[[buffer(4)]],uint i[[thread_position_in_grid]]){
 if(i>=cfg[1]||sv[i*STRIDE+1]==2)return;uint3 s=cu_schema(cfg[0]);uint t=uint(sv[i*STRIDE]);
 if(t==s.x)costs[atomic_fetch_add_explicit((device atomic_uint*)(c+8),1u,memory_order_relaxed)]=i;
 if(t==s.y)choices[atomic_fetch_add_explicit((device atomic_uint*)(c+9),1u,memory_order_relaxed)]=i;
}
kernel void cu_reset(CU_ARGS,device uint*ranges[[buffer(3)]],device uint*heads[[buffer(4)]],uint i[[thread_position_in_grid]]){
 if(c[2])return;if(i<2*cfg[2])ranges[i]=0;if(i<cfg[5])heads[i]=0;if(!i)c[5]=0;
}
template<typename R> void cu_extreme(device atomic_uint*at,uint row,R r,bool maximum){
 uint old=atomic_load_explicit(at,memory_order_relaxed);long value=r[row*STRIDE+5];
 while(true){if(old){long prev=r[(old-1)*STRIDE+5];if((maximum?prev>value:prev<value)||(prev==value&&old<=row+1))return;}
 uint expected=old;if(atomic_compare_exchange_weak_explicit(at,&expected,row+1,memory_order_relaxed,memory_order_relaxed))return;old=expected;}
}
kernel void cu_reduce(CU_ARGS,device const uint*costs[[buffer(3)]],device const uint*p[[buffer(4)]],device uint*ranges[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(c[2]||i>=cfg[3])return;uint row=costs[i];if(sv[row*STRIDE+1])return;uint id=cu_root(p,uint(sv[row*STRIDE+6]));
 cu_extreme((device atomic_uint*)(ranges+2*id),row,sv,false);cu_extreme((device atomic_uint*)(ranges+2*id+1),row,sv,true);
}
kernel void cu_gather(CU_ARGS,device const uint*choices[[buffer(3)]],device const uint*p[[buffer(4)]],device const uint*ranges[[buffer(5)]],device uint*ports[[buffer(6)]],device half*x[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(c[2]||i>=cfg[4])return;uint row=choices[i];if(sv[row*STRIDE+1]){ports[4*i]=0xffffffff;ports[4*i+1]=0xffffffff;ports[4*i+2]=0;ports[4*i+3]=0;for(uint j=0;j<32;j++)x[32*i+j]=0;return;}uint a=cu_root(p,uint(sv[row*STRIDE+4])),b=cu_root(p,uint(sv[row*STRIDE+5]));
 ports[4*i]=a;ports[4*i+1]=b;ports[4*i+2]=cu_root(p,uint(sv[row*STRIDE+6]));ports[4*i+3]=ranges[2*a]&&ranges[2*b];
 for(uint j=0;j<4;j++){uint witness=ranges[2*(j<2?a:b)+(j&1)];ulong bits=witness?(as_type<ulong>(sv[(witness-1)*STRIDE+5])^0x8000000000000000UL):0;
  for(uint k=0;k<8;k++)x[32*i+8*j+k]=half((bits>>(56-8*k))&255);}
}
kernel void cu_mma(CU_ARGS,device const half*x[[buffer(3)]],device const half*w[[buffer(4)]],device float*y[[buffer(5)]],uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 if(c[2])return;uint row=(tile/2)*8,col=(tile%2)*8;threadgroup half a[64],b[64];threadgroup float out[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<32;k+=8){for(uint q=lane;q<64;q+=32){uint rr=q/8,cc=q%8;a[q]=row+rr<cfg[4]?x[(row+rr)*32+k+cc]:half(0);b[q]=w[(k+rr)*16+col+cc];}
 threadgroup_barrier(mem_flags::mem_threadgroup);simdgroup_half8x8 aa,bb;simdgroup_load(aa,a,8);simdgroup_load(bb,b,8);simdgroup_multiply_accumulate(acc,aa,bb,acc);threadgroup_barrier(mem_flags::mem_threadgroup);}
 simdgroup_store(acc,out,8);threadgroup_barrier(mem_flags::mem_threadgroup);for(uint q=lane;q<64;q+=32){uint rr=row+q/8,cc=col+q%8;if(rr<cfg[4])y[rr*16+cc]=out[q];}
}
int cu_sign(device const float*y,uint at){for(uint j=0;j<8;j++){float v=y[at+j];if(v>0)return 1;if(v<0)return -1;}return 0;}
kernel void cu_priority(CU_ARGS,device const uint*choices[[buffer(3)]],device const uint*p[[buffer(4)]],device const uint*classes[[buffer(5)]],device uint*priority[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i>=cfg[4])return;uint row=choices[i];priority[i]=sv[row*STRIDE+1]?0:classes[cu_root(p,uint(sv[row*STRIDE+6]))];
}
kernel void cu_apply(CU_ARGS,device const uint*ports[[buffer(3)]],device const float*y[[buffer(4)]],device uint*p[[buffer(5)]],device const uint*ranges[[buffer(6)]],device const uint*priority[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(c[2]||i>=cfg[4]||!ports[4*i+3])return;uint a=ports[4*i],b=ports[4*i+1],out=ports[4*i+2];bool rightTie=cu_schema(cfg[0]).z;
 if(cfg[7]&&priority[i]!=uint(cfg[7]==1))return;
 int l=cu_sign(y,16*i),rr=cu_sign(y,16*i+8);bool left=l>0||(l==0&&!rightTie),right=rr>0||(rr==0&&rightTie);
 if(cfg[6]){long amin=sv[(ranges[2*a]-1)*STRIDE+5],amax=sv[(ranges[2*a+1]-1)*STRIDE+5],bmin=sv[(ranges[2*b]-1)*STRIDE+5],bmax=sv[(ranges[2*b+1]-1)*STRIDE+5];
  if(left!=(rightTie?amin<bmax:amin<=bmax)||right!=(rightTie?bmin<=amax:bmin<amax)){atomic_store_explicit((device atomic_uint*)(c+6),2u,memory_order_relaxed);return;}}
 if(left)cu_union(p,out,a,c,r);if(right)cu_union(p,out,b,c,r);
}
kernel void cu_congruence(CU_ARGS,device const uint*ports[[buffer(3)]],device uint*heads[[buffer(4)]],device uint*p[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(c[2]||i>=cfg[4]||ports[4*i]==0xffffffff)return;uint a=ports[4*i],b=ports[4*i+1],slot=(a*0x9e3779b9u+b*0x85ebca6bu)&(cfg[5]-1);device atomic_uint*h=(device atomic_uint*)heads;
 while(true){uint old=atomic_load_explicit(h+slot,memory_order_relaxed);if(!old){uint expected=0;if(atomic_compare_exchange_weak_explicit(h+slot,&expected,i+1,memory_order_relaxed,memory_order_relaxed))return;}
 else if(ports[4*(old-1)]==a&&ports[4*(old-1)+1]==b){cu_union(p,ports[4*i+2],ports[4*(old-1)+2],c,r);return;}
 else slot=(slot+1)&(cfg[5]-1);}
}
kernel void cu_finish(CU_ARGS,uint i[[thread_position_in_grid]]){
 if(i||c[2])return;c[3]++;c[4]+=c[5];if(!c[5]||c[6])c[2]=1;if(c[3]>cfg[2]+1){c[6]=3;c[2]=1;}
}
kernel void cu_publish(CU_ARGS,device const uint*local[[buffer(3)]],device uint*p[[buffer(4)]],device Meta&m[[buffer(5)]],device uint*wm[[buffer(6)]],device uint*control[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(c[6])return;
 if(c[4]&&i<cfg[2]){uint to=cu_root(local,i);p[i]=to;
  device atomic_uint*flags=(device atomic_uint*)(p+MERGE_KEY_FLAGS);
  if(atomic_load_explicit(flags+i,memory_order_relaxed))atomic_store_explicit(flags+to,1u,memory_order_relaxed);}
 if(!i){if(JP_BLOCK_EFFECTS&&jp_active(r)){r[JP_STATE+114]++;r[JP_STATE+115]+=c[4];r[JP_STATE+116]++;}m.epoch++;m.rounds++;m.changed=c[4]>0;m.matched=0;m.unions+=c[4];p[IDCAP]=0;p[IDCAP+1]=0;
  if(c[4]){p[MERGE_GENERATION]+=c[4];r[INDEX_OFFSET]=0;r[QUERY_DIRTY]=1;}
  r[ROW_WORDS+1]=m.epoch;for(uint j=SET_START[cfg[0]];j<SET_START[cfg[0]+1];j++)wm[SET_RULES[j]]=m.epoch;
  for(uint j=0;j<16;j++)control[j]=0;control[0]=c[4]>0;
 }
}

// A completed local equality block contributes edges, never a replacement UF.
// Therefore another block's committed union cannot be overwritten by a stale
// local parent array. Positive cost evidence remains valid under more equality.
kernel void cu_block_begin(CU_ARGS,device Meta&m[[buffer(3)]],uint i[[thread_position_in_grid]]){
 if(i||c[6])return;c[12]=m.unions;m.changed=0;
}
kernel void cu_block_union(CU_ARGS,device const uint2*edges[[buffer(3)]],device uint*p[[buffer(4)]],device Meta&m[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(c[6]||i>=cfg[3])return;uint a=edges[i].x,b=edges[i].y;device atomic_uint*uf=(device atomic_uint*)p;
 while(true){a=parallel_root(uf,a);b=parallel_root(uf,b);if(a==b)return;uint lo=min(a,b),hi=max(a,b),expected=hi;
  if(atomic_compare_exchange_weak_explicit(uf+hi,&expected,lo,memory_order_relaxed,memory_order_relaxed)){
   atomic_fetch_add_explicit((device atomic_uint*)&m.unions,1u,memory_order_relaxed);
   if(jp_active(r))atomic_fetch_max_explicit((device atomic_uint*)(r+JP_VERSION+lo),m.epoch+1,memory_order_relaxed);return;
  }
 }
}
kernel void cu_block_finish(CU_ARGS,device uint*p[[buffer(3)]],device Meta&m[[buffer(4)]],device uint*wm[[buffer(5)]],device uint*control[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(c[6])return;
 if(i<cfg[2]){device atomic_uint*uf=(device atomic_uint*)p;uint to=parallel_root(uf,i);atomic_store_explicit(uf+i,to,memory_order_relaxed);device atomic_uint*flags=uf+MERGE_KEY_FLAGS;if(atomic_load_explicit(flags+i,memory_order_relaxed))atomic_store_explicit(flags+to,1u,memory_order_relaxed);}
 if(!i){uint added=m.unions-c[12];c[13]=added;m.epoch++;m.rounds++;m.changed=added>0;m.matched=0;p[IDCAP]=0;p[IDCAP+1]=0;
  if(added){p[MERGE_GENERATION]+=added;r[INDEX_OFFSET]=0;r[QUERY_DIRTY]=1;}
  if(jp_active(r)){r[JP_STATE+114]++;r[JP_STATE+115]+=added;r[JP_STATE+116]++;}
  r[ROW_WORDS+1]=m.epoch;for(uint j=SET_START[cfg[0]];j<SET_START[cfg[0]+1];j++)wm[SET_RULES[j]]=m.epoch;
  for(uint j=0;j<16;j++)control[j]=0;control[0]=added>0;
 }
}

// A basis for the new equivalence information: one edge for each old root
// that ceased to be a root. Old aliases need no duplicate record.
kernel void cu_block_delta(CU_ARGS,device const uint*before[[buffer(3)]],device const uint*after[[buffer(4)]],device uint2*edges[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(!i&&c[2]&&!c[6]&&jp_active(r))r[JP_STATE+123]+=r[JP_STATE+109];
 if(!c[2]||c[6]||i>=cfg[2]||before[i]!=i)return;uint to=cu_root(after,i);if(to==i)return;
 uint at=atomic_fetch_add_explicit((device atomic_uint*)(c+14),1u,memory_order_relaxed);if(at>=cfg[2]){atomic_store_explicit((device atomic_uint*)(c+6),5u,memory_order_relaxed);return;}edges[at]=uint2(i,to);
}
