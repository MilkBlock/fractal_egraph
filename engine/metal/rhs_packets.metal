// Pure construction slices use symbolic references until the whole DAG has
// been interned. No global table write occurs before preparation succeeds.
#define RP_STRIDE (COLS+3)
#define RP_BLOCK 64
#define RP_OWNER (RP_BLOCK+(w[63]+255)/256)
#define RP_PREFIX (RP_OWNER+w[63])
#define RP_HASH (RP_PREFIX+w[63])
#define RP_FIRST (RP_HASH+2*w[63])
#define RP_FOUND (RP_FIRST+w[63])
#define RP_WORDS (64+(writeCfg.x+255)/256+6*writeCfg.x)
uint rp_root(device const uint*p,uint x){while(p[x]!=x)x=p[x];return x;}
long rp_existing(uint row,device long*r,device const uint*p){
 uint t=uint(r[row*STRIDE]);long id=(row>=uint(r[QUERY_LOG+1])&&r[CONSTRUCTOR_PRED+row])?r[CONSTRUCTOR_PRED+row]:r[row*STRIDE+4+WIDTH[t]-1];
 return long(rp_root(p,uint(id)));
}
long rp_result(uint at,device long*packets,device uint*w,device long*r,device uint*p){
 return w[RP_FOUND+at]!=0xffffffff?rp_existing(w[RP_FOUND+at],r,p):packets[w[RP_OWNER+at]*RP_STRIDE+2];
}
// RHS_PACKET_RECIPES
#define RP_ARGS BC_ARGS,uint2 point[[thread_position_in_grid]]
#define RP_LOCAL \
 uint instance=point.y,lane=point.x;if(instance>=cfg.x)return; \
 device Meta&m=all_meta[instance];device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE)); \
 if(m.error||stats[10]!=9)return; \
 device long*r=all_rows+instance*GC_ROW_WORDS;device uint*p=all_uint+instance*GC_UINT_WORDS; \
 device uint*fp=all_uint+instance*GC_UINT_WORDS+GC_FP;device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL; \
 device long*matches=all_long+instance*GC_LONG_WORDS+GC_MATCHES; \
 device uint*s=all_state+instance*GC_STATE_WORDS; \
 device uint*order=all_uint+instance*GC_UINT_WORDS+GC_MATCH_ORDER+bc_order_offset(m.matched,bc_requires_sort(tape[4*s[0]+1])); \
 device long*meta=stats+BC_STATS;device long*data=meta+3*batchCfg.x; \
 device uint*w=all_write_uint+instance*RP_WORDS;device long*packets=all_write_long+instance*writeCfg.x*RP_STRIDE;
int rp_plan(uint ordinal,device uint*control,device long*meta){
 if(!control[37])return -1;uint lo=control[31],hi=control[32];
 while(lo+1<hi){uint mid=(lo+hi)/2;if(uint(meta[3*mid])<=ordinal)lo=mid;else hi=mid;}return int(meta[3*lo+2]);
}
bool rp_done(int plan,device long*o){for(uint i=0;i<rp_cell_steps(plan);i++)if(o[2*BC_WIDTH+i]!=1)return false;return true;}
bool rp_partial(int plan,device long*o){for(uint i=0;i<rp_cell_steps(plan);i++)if(o[2*BC_WIDTH+i]!=0)return true;return false;}
uint rp_hash(device long*a){uint t=uint(a[0]);ulong h=ulong(t)+0x9e3779b97f4a7c15UL;for(uint i=0;i<WIDTH[t]-1;i++){ulong v=ulong(a[3+i]);v^=v>>30;v*=0xbf58476d1ce4e5b9UL;h^=v+(h<<6)+(h>>2);}return uint(h^(h>>32));}
bool rp_equal(device long*a,device long*b){uint t=uint(a[0]);if(t!=uint(b[0]))return false;for(uint i=0;i<WIDTH[t]-1;i++)if(a[3+i]!=b[3+i])return false;return true;}

kernel void resident_packet_gate(RP_ARGS){
 RP_LOCAL;if(lane)return;
 if(w[63]!=writeCfg.x){for(uint i=0;i<64;i++)w[i]=0;w[63]=writeCfg.x;w[37]=control[24]-control[23];
  uint first=control[23],rule=uint(matches[order[first]*MSTRIDE]);int plan=rp_plan(first,control,meta);
  w[48]=plan>=0?rp_cell_writes(plan):RP_WRITES[rule];w[47]=plan>=0?rp_cell_depth(plan):RP_DEPTH[rule];
  ulong required=ulong(w[37])*w[48];if(required>writeCfg.x){w[43]=uint(min(required,ulong(0xffffffff)));m.error=40;return;}
  w[1]=uint(required);uint size=2;while(size<2*w[1])size*=2;w[3]=size;
 }
 atomic_fetch_max_explicit(dispatch+BC_WRITE_BINDINGS,(w[37]+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_WRITE_ONE,1u,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_WRITE_EFFECTS,(w[1]+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_WRITE_HASH,(w[3]+255)/256,memory_order_relaxed);
}
// Cohorts share one rule/template, so offsets are multiplication, not a scan.
// Hash storage and request records are disjoint: clearing and preparing can run
// in one dispatch. No-op slots remain inactive and are never published.
kernel void resident_packet_prepare(RP_ARGS){
 RP_LOCAL;if(w[49])return;
 if(lane<w[3])w[RP_HASH+lane]=0;
 if(lane<w[1]){w[RP_OWNER+lane]=0xffffffff;w[RP_FIRST+lane]=lane;w[RP_FOUND+lane]=0xffffffff;}
 if(lane>=w[37])return;uint ordinal=control[23]+lane,b=order[ordinal],rule=uint(matches[b*MSTRIDE]);int plan=rp_plan(ordinal,control,meta);
 bool skip=plan>=0?rp_done(plan,data+ordinal*BC_STRIDE):(NOOP_PREFLIGHT&&(control[36]?all_uint[instance*GC_UINT_WORDS+GC_NOOP+b]:data[ordinal*BC_STRIDE+BC_FAST]!=0));
 if(skip){for(uint i=0;i<w[48];i++)packets[(lane*w[48]+i)*RP_STRIDE+1]=-1;atomic_fetch_add_explicit((device atomic_uint*)(w+45),w[48],memory_order_relaxed);if(plan<0)atomic_fetch_add_explicit((device atomic_uint*)(w+44),1u,memory_order_relaxed);return;}
 if(plan>=0&&rp_partial(plan,data+ordinal*BC_STRIDE)){atomic_store_explicit((device atomic_uint*)w,1u,memory_order_relaxed);return;}
 if(!rp_recipe(plan,b,matches,control[37]?data+ordinal*BC_STRIDE:data,packets,w,r,p,fp,m,lane*w[48],false))atomic_store_explicit((device atomic_uint*)w,1u,memory_order_relaxed);
}
kernel void resident_packet_keys(RP_ARGS){
 RP_LOCAL;if(w[49]>=2||w[0]||lane>=w[1])return;device long*a=packets+lane*RP_STRIDE;if(uint(a[1])!=w[46])return;
 uint t=uint(a[0]);bool existing=true;long key[COLS]={0};
 for(uint i=0;i<WIDTH[t]-1;i++){long value=a[3+i];if(EQ[t*COLS+i]==1){
   if(value<0){uint dep=uint(-1-value);if(w[RP_FOUND+dep]!=0xffffffff)value=rp_existing(w[RP_FOUND+dep],r,p);else value=-1L-long(w[RP_OWNER+dep]);}
   else value=long(rp_root(p,uint(value)));if(value<0)existing=false;
  }a[3+i]=key[i]=value;
 }
 if(existing){long row=query_lookup(r,t,key);if(row>=0){
   if((uint(row)>=uint(r[QUERY_LOG+1])&&!r[CONSTRUCTOR_PRED+row])||(control[37]&&r[row*STRIDE+1]!=0)){atomic_store_explicit((device atomic_uint*)w,1u,memory_order_relaxed);return;}
   w[RP_FOUND+lane]=uint(row);w[RP_OWNER+lane]=lane;cw_add(stats,69);
 }}
}
kernel void resident_packet_group(RP_ARGS){
 RP_LOCAL;if(w[49]>=2||w[0]||lane>=w[1]||w[RP_FOUND+lane]!=0xffffffff)return;device long*a=packets+lane*RP_STRIDE;if(uint(a[1])!=w[46])return;
 device atomic_uint*heads=(device atomic_uint*)(w+RP_HASH);uint h=rp_hash(a)&(w[3]-1),owner;
 while(true){uint old=atomic_load_explicit(heads+h,memory_order_relaxed);if(!old){uint zero=0;if(atomic_compare_exchange_weak_explicit(heads+h,&zero,lane+1,memory_order_relaxed,memory_order_relaxed)){owner=lane;break;}}
  else if(rp_equal(a,packets+(old-1)*RP_STRIDE)){owner=old-1;break;}else h=(h+1)&(w[3]-1);}
 w[RP_OWNER+lane]=owner;atomic_fetch_min_explicit((device atomic_uint*)(w+RP_FIRST+owner),lane,memory_order_relaxed);
}
kernel void resident_packet_next(RP_ARGS){
 RP_LOCAL;if(lane||w[49]>=2)return;cw_add(stats,63);w[46]++;w[49]=(w[0]||w[46]>=w[47])?2:1;
}
kernel void resident_packet_scan(RP_ARGS,uint tid[[thread_index_in_threadgroup]]){
 RP_LOCAL;if(w[49]!=2||w[0])return;threadgroup uint sums[256];uint live=0;
 if(lane<w[1])live=packets[lane*RP_STRIDE+1]>=0&&w[RP_FOUND+lane]==0xffffffff&&w[RP_FIRST+w[RP_OWNER+lane]]==lane;
 sums[tid]=live;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint x=tid>=d?sums[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sums[tid]+=x;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<w[1])w[RP_PREFIX+lane]=sums[tid]-live;if(tid==255)w[RP_BLOCK+lane/256]=sums[tid];
}
kernel void resident_packet_reserve(RP_ARGS){
 RP_LOCAL;if(lane||w[49]!=2||w[0])return;uint total=0;
 for(uint i=0;i<(w[1]+255)/256;i++){uint x=w[RP_BLOCK+i];w[RP_BLOCK+i]=total;total+=x;}
 // Bounds include all possible ordered callback allocations. On shortage the
 // original executor preserves its partial-error behavior; no packets escape.
 uint rule=uint(matches[order[control[23]]*MSTRIDE]);ulong extraRows=control[37]?0:ulong(w[37])*RP_TAIL_ROWS[rule],extraIds=control[37]?0:ulong(w[37])*RP_TAIL_IDS[rule];
 if(ulong(m.count)+total+extraRows>CAPACITY||ulong(m.next_fact)+total+extraRows>IDCAP||ulong(m.next_id)+total+extraIds>IDCAP||(!control[37]&&ulong(p[IDCAP+1])+ulong(w[37])*RP_TAIL_UNIONS[rule]>UNION_CAP)){w[0]=1;return;}
 w[4]=total;w[5]=m.count;w[6]=m.next_id;w[7]=m.next_fact;m.count+=total;m.next_id+=total;m.next_fact+=total;if(total)m.changed=1;
 cw_add(stats,62,total);
}
kernel void resident_packet_ids(RP_ARGS){
 RP_LOCAL;if(w[49]!=2||w[0]||lane>=w[1]||w[RP_FOUND+lane]!=0xffffffff||w[RP_OWNER+lane]!=lane)return;
 uint first=w[RP_FIRST+lane],n=w[RP_BLOCK+first/256]+w[RP_PREFIX+first];packets[lane*RP_STRIDE+2]=long(w[6]+n);
}
kernel void resident_packet_publish(RP_ARGS){
 RP_LOCAL;if(w[49]!=2||w[0]||lane>=w[1]||w[RP_FOUND+lane]!=0xffffffff||w[RP_OWNER+lane]!=lane)return;
 uint first=w[RP_FIRST+lane],n=w[RP_BLOCK+first/256]+w[RP_PREFIX+first],row=w[5]+n,id=w[6]+n,fid=w[7]+n;device long*a=packets+lane*RP_STRIDE;uint t=uint(a[0]);
 p[id]=id;p[MERGE_KEY_FLAGS+id]=MERGE_KEY_OUTPUT[t];fp[fid]=fid;
 r[row*STRIDE]=t;r[row*STRIDE+1]=0;r[row*STRIDE+2]=m.epoch;r[row*STRIDE+3]=fid;
 ulong hash=ulong(t)+0x9e3779b97f4a7c15UL;
 for(uint i=0;i<WIDTH[t]-1;i++){long v=a[3+i];if(EQ[t*COLS+i]==1&&v<0)v=packets[uint(-1-v)*RP_STRIDE+2];r[row*STRIDE+4+i]=v;ulong bits=ulong(v);bits^=bits>>30;bits*=0xbf58476d1ce4e5b9UL;hash^=bits+(hash<<6)+(hash>>2);}
 r[row*STRIDE+4+WIDTH[t]-1]=id;r[CONSTRUCTOR_PRED+row]=id;r[RC_FACT_ROW+fid]=row+1;rc_fact_dirty(r,row);atomic_fetch_max_explicit((device atomic_uint*)(r+RC_VERSION+t),m.epoch,memory_order_relaxed);
 uint h=uint(hash^(hash>>32))&(HASH_CAP-1);device atomic_uint*heads=(device atomic_uint*)(r+INDEX_OFFSET+1);
 while(true){uint old=atomic_load_explicit(heads+2*h,memory_order_relaxed);if(!old){uint zero=0;if(atomic_compare_exchange_weak_explicit(heads+2*h,&zero,row+1,memory_order_relaxed,memory_order_relaxed)){uint at=atomic_fetch_add_explicit((device atomic_uint*)(r+RC_INDEX_TOUCH),1u,memory_order_relaxed);r[RC_INDEX_TOUCH+1+at]=h;return;}}else h=(h+1)&(HASH_CAP-1);}
}
kernel void resident_packet_reply(RP_ARGS){
 RP_LOCAL;if(w[49]!=2||w[0]||!control[37]||lane>=w[37])return;uint ordinal=control[23]+lane,b=order[ordinal];int plan=rp_plan(ordinal,control,meta);device long*o=data+ordinal*BC_STRIDE;if(rp_done(plan,o))return;
 // All bindings in this cohort use the same immutable request layout.
 uint base=lane*w[48];
 if(!rp_recipe(plan,b,matches,o,packets,w,r,p,fp,m,base,true)){atomic_store_explicit((device atomic_uint*)&m.error,5u,memory_order_relaxed);return;}
 cw_add(stats,1);cw_add(stats,2,rp_cell_steps(plan)-1);cw_add(stats,4,rp_cell_effects(plan));cw_add(stats,66);cw_add(stats,67,rp_cell_steps(plan));
}
kernel void resident_packet_finish(RP_ARGS){
 RP_LOCAL;if(lane||w[49]!=2)return;
 if(w[0]){cw_add(stats,65);for(uint i=0;i<w[37]&&!m.error;i++){uint ordinal=control[23]+i,b=order[ordinal];int plan=rp_plan(ordinal,control,meta);
   if(plan>=0){device long*o=data+ordinal*BC_STRIDE;bc_publish(plan,b,matches,o,r,p,fp,m,stats);stats[21]+=bc_waiting(plan,o);}
   else{execute_actions(uint(matches[b*MSTRIDE]),b,code,all_long+instance*GC_LONG_WORDS+GC_REGISTERS,matches,r,p,fp,m);stats[5]++;if(control[36])stats[58]++;}
  }}else{
  cw_add(stats,60,w[37]);cw_add(stats,61,w[1]-w[45]);cw_add(stats,70,w[1]-w[45]-w[4]);
  if(!control[37]){uint rule=uint(matches[order[control[23]]*MSTRIDE]);if(RP_TAIL[rule]){
    for(uint i=0;i<w[37]&&!m.error;i++){uint ordinal=control[23]+i,b=order[ordinal];bool skip=NOOP_PREFLIGHT&&(control[36]?all_uint[instance*GC_UINT_WORDS+GC_NOOP+b]:data[ordinal*BC_STRIDE+BC_FAST]!=0);if(skip)continue;
     if(!rp_recipe(-1,b,matches,data,packets,w,r,p,fp,m,i*w[48],true)&&!m.error)m.error=5;cw_add(stats,64);
    }}stats[5]+=w[37];stats[28]+=w[44];if(control[36])stats[58]+=w[37];else stats[43]+=w[37];
  }
 }
 bool certificates=control[37]&&!w[0];w[63]=0;
 if(certificates)stats[10]=11;else{control[27]=control[24];stats[10]=10;}
}
