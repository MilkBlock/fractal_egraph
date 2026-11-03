// Temporary packets, not a second authoritative graph/context cache. Contexts
// live in batchData; each request carries an exact key and a continuation slot.
#define CW_STRIDE (COLS+12)
#define CW_HASH 128
#define CW_LEADER (CW_HASH+2*batchCfg.x)
#define CW_NEXT (CW_LEADER+batchCfg.x)
#define CW_HEAD (CW_NEXT+batchCfg.x)
#define CW_RP (CW_HEAD+batchCfg.x)
#define CW_IP (CW_RP+batchCfg.x)
#define CW_UP (CW_IP+batchCfg.x)
#define CW_BLOCK (CW_UP+batchCfg.x)
#define CW_WORDS (CW_BLOCK+3*((batchCfg.x+255)/256))
#define CW_ARGS BC_ARGS,device uint*wave_u[[buffer(19)]],device long*wave_p[[buffer(20)]],uint2 point[[thread_position_in_grid]]
#define CW_LOCAL \
 uint instance=point.y,lane=point.x;if(instance>=cfg.x)return; \
 device Meta&m=all_meta[instance];device long*r=all_rows+instance*GC_ROW_WORDS; \
 device uint*p=all_uint+instance*GC_UINT_WORDS; \
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL; \
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE)); \
 if(m.error||stats[10]!=11)return; \
 device uint*w=wave_u+instance*CW_WORDS; \
 device long*packets=wave_p+instance*batchCfg.x*CW_STRIDE; \
 device long*meta=stats+BC_STATS;device long*data=meta+3*batchCfg.x;
uint cw_hash(device long*a){uint t=uint(a[0]);ulong h=ulong(t)+0x9e3779b97f4a7c15UL;for(uint j=0;j<WIDTH[t]-1;j++){ulong v=ulong(a[12+j]);v^=v>>30;v*=0xbf58476d1ce4e5b9UL;h^=v+(h<<6)+(h>>2);}return uint(h^(h>>32));}
bool cw_equal(device long*a,device long*b){uint t=uint(a[0]);if(b[0]!=t)return false;for(uint j=0;j<WIDTH[t]-1;j++)if(a[12+j]!=b[12+j])return false;return true;}
void cw_changed(device long*r,uint row,uint epoch){
 uint t=uint(r[row*STRIDE]);rc_fact_dirty(r,row);r[RC_FACT_ROW+uint(r[row*STRIDE+3])]=row+1;
 atomic_fetch_max_explicit((device atomic_uint*)(r+RC_VERSION+t),epoch,memory_order_relaxed);
}
// Unique key leaders exclusively own their old row. Only journal allocation
// and table-wide version publication need atomics.
void cw_record_update(device long*r,uint row){
 if(!QUERY_PATCH){atomic_store_explicit((device atomic_uint*)(r+QUERY_DIRTY),1u,memory_order_relaxed);return;}
 if(r[QUERY_DIRTY]==1||row>=uint(r[QUERY_LOG+1])||r[row*STRIDE+1]!=0)return;
 if(!r[QUERY_OLD_TIME+row]){uint at=atomic_fetch_add_explicit((device atomic_uint*)(r+QUERY_LOG),1u,memory_order_relaxed);r[QUERY_OLD_VALUE+row]=r[row*STRIDE+4+WIDTH[uint(r[row*STRIDE])]-1];r[QUERY_OLD_TIME+row]=r[row*STRIDE+2];r[QUERY_CHANGED+at]=row;}
 atomic_store_explicit((device atomic_uint*)(r+QUERY_DIRTY),2u,memory_order_relaxed);
}
kernel void cell_wave_gate(CW_ARGS){
 CW_LOCAL;if(lane)return;
 for(uint i=0;i<128;i++)w[i]=0;w[1]=0xffffffff;w[10]=control[24]-control[23];
 uint size=2;while(size<2*w[10])size*=2;w[9]=size;
 atomic_fetch_max_explicit(dispatch+BC_WAVE_ROWS,(w[10]+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_WAVE_HASH,(size+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_WAVE_ONE,1u,memory_order_relaxed);
 cw_add(stats,49);
}
kernel void cell_wave_clear(CW_ARGS){CW_LOCAL;if(lane<w[9])w[CW_HASH+lane]=0;if(lane<w[10])w[CW_HEAD+lane]=0;}
kernel void cell_wave_propose(CW_ARGS){
 CW_LOCAL;if(lane>=w[10])return;uint ordinal=control[23]+lane,lo=control[31],hi=control[32];
 while(lo+1<hi){uint mid=(lo+hi)/2;if(uint(meta[3*mid])<=ordinal)lo=mid;else hi=mid;}
 int plan=int(meta[3*lo+2]);device long*out=packets+lane*CW_STRIDE;out[10]=plan;out[11]=ordinal;
 cw_propose(plan,data+ordinal*BC_STRIDE,out,r,p,stats);
 if(out[0]==-3)atomic_fetch_min_explicit((device atomic_uint*)(w+1),lane,memory_order_relaxed);
 else if(out[0]!=-1)atomic_fetch_add_explicit((device atomic_uint*)w,1u,memory_order_relaxed);
}
kernel void cell_wave_group(CW_ARGS){
 CW_LOCAL;if(lane>=w[10]||w[1]!=0xffffffff)return;device long*a=packets+lane*CW_STRIDE;if(a[0]<0)return;
 device atomic_uint*heads=(device atomic_uint*)(w+CW_HASH);uint h=cw_hash(a)&(w[9]-1);
 while(true){uint old=atomic_load_explicit(heads+h,memory_order_relaxed);if(!old){uint expected=0;if(atomic_compare_exchange_weak_explicit(heads+h,&expected,lane+1,memory_order_relaxed,memory_order_relaxed))return;}
 else if(cw_equal(a,packets+(old-1)*CW_STRIDE)){atomic_fetch_min_explicit(heads+h,lane+1,memory_order_relaxed);return;}else h=(h+1)&(w[9]-1);}
}
kernel void cell_wave_link(CW_ARGS){
 CW_LOCAL;if(lane>=w[10]||w[1]!=0xffffffff)return;device long*a=packets+lane*CW_STRIDE;w[CW_LEADER+lane]=0xffffffff;
 if(a[0]<0)return;uint h=cw_hash(a)&(w[9]-1);while(!cw_equal(a,packets+(w[CW_HASH+h]-1)*CW_STRIDE))h=(h+1)&(w[9]-1);
 uint leader=w[CW_HASH+h]-1;w[CW_LEADER+lane]=leader;
 w[CW_NEXT+lane]=atomic_exchange_explicit((device atomic_uint*)(w+CW_HEAD+leader),lane+1,memory_order_relaxed);
}
kernel void cell_wave_scan(CW_ARGS,uint tid[[thread_index_in_threadgroup]]){
 CW_LOCAL;if(w[1]!=0xffffffff)return;threadgroup uint3 sums[256];uint3 live(0);
 if(lane<w[10]){device long*a=packets+lane*CW_STRIDE;uint lead=w[CW_LEADER+lane];
  if(a[0]>=0){device long*first=packets+lead*CW_STRIDE;live.x=uint(lead==lane&&a[2]<0);live.y=live.x&&KIND[uint(a[0])]==0&&a[1]==0;
   if(a[1]==1&&KIND[uint(a[0])]==0){bool fresh=a[2]<0&&first[1]==0;long base=a[2]>=0?a[7]:first[3];live.z=fresh||a[3]!=base;}}
  else if(a[0]==-2)live.z=a[3]!=a[4];
 }
 sums[tid]=live;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint3 v=tid>=d?sums[tid-d]:uint3(0);threadgroup_barrier(mem_flags::mem_threadgroup);sums[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<w[10]){w[CW_RP+lane]=sums[tid].x-live.x;w[CW_IP+lane]=sums[tid].y-live.y;w[CW_UP+lane]=sums[tid].z-live.z;packets[lane*CW_STRIDE+8]=live.z;}
 if(tid==255){uint at=CW_BLOCK+3*(lane/256);w[at]=sums[tid].x;w[at+1]=sums[tid].y;w[at+2]=sums[tid].z;}
}
kernel void cell_wave_reserve(CW_ARGS){
 CW_LOCAL;if(lane)return;if(w[1]!=0xffffffff){m.error=uint(packets[w[1]*CW_STRIDE+9]);return;}
 uint3 n(0);for(uint i=0;i<(w[10]+255)/256;i++){uint at=CW_BLOCK+3*i;uint3 v(w[at],w[at+1],w[at+2]);w[at]=n.x;w[at+1]=n.y;w[at+2]=n.z;n+=v;}
 if(ulong(m.count)+n.x>CAPACITY||ulong(m.next_fact)+n.x>IDCAP||ulong(m.next_id)+n.y>IDCAP){m.error=1;return;}
 if(ulong(p[IDCAP+1])+n.z>UNION_CAP){m.error=11;return;}
 w[2]=n.x;w[3]=n.y;w[4]=n.z;w[5]=m.count;w[6]=m.next_id;w[7]=m.next_fact;w[8]=p[IDCAP+1];
 m.count+=n.x;m.next_fact+=n.x;m.next_id+=n.y;p[IDCAP+1]+=n.z;if(n.x)m.changed=1;
 cw_add(stats,46,n.x);cw_add(stats,52,n.z);
}
kernel void cell_wave_publish(CW_ARGS){
 CW_LOCAL;if(lane>=w[10]||w[CW_LEADER+lane]!=lane)return;
 device long*a=packets+lane*CW_STRIDE;uint t=uint(a[0]),block=CW_BLOCK+3*(lane/256),row=a[2]>=0?uint(a[2]):w[5]+w[block]+w[CW_RP+lane];
 long result=a[2]>=0?a[7]:(KIND[t]==0&&a[1]==0?long(w[6]+w[block+1]+w[CW_IP+lane]):a[3]);
 uint members=0;for(uint member=w[CW_HEAD+lane];member;member=w[CW_NEXT+member-1]){device long*b=packets+(member-1)*CW_STRIDE;members++;if(KIND[t]!=0){if(POLICY[t]==1)result=min(result,b[3]);else if(POLICY[t]==2)result=max(result,b[3]);}}
 cw_add(stats,47);cw_add(stats,48,members-1);a[4]=result;
 if(a[2]<0){uint fid=w[7]+w[block]+w[CW_RP+lane];
  if(KIND[t]==0&&a[1]==0){p[uint(result)]=uint(result);p[MERGE_KEY_FLAGS+uint(result)]=MERGE_KEY_OUTPUT[t];}
  device uint*fp=all_uint+instance*GC_UINT_WORDS+GC_FP;fp[fid]=fid;
  r[row*STRIDE]=t;r[row*STRIDE+1]=0;r[row*STRIDE+2]=m.epoch;r[row*STRIDE+3]=fid;
  for(uint j=0;j<WIDTH[t]-1;j++)r[row*STRIDE+4+j]=a[12+j];r[row*STRIDE+4+WIDTH[t]-1]=result;r[CONSTRUCTOR_PRED+row]=KIND[t]==0&&a[1]==0?result:0;
  cw_changed(r,row,m.epoch);
 }else if(KIND[t]!=0&&r[row*STRIDE+4+WIDTH[t]-1]!=result){
  cw_record_update(r,row);r[row*STRIDE+4+WIDTH[t]-1]=result;r[row*STRIDE+2]=m.epoch;cw_changed(r,row,m.epoch);
  atomic_store_explicit((device atomic_uint*)&m.changed,1u,memory_order_relaxed);
 }
}
kernel void cell_wave_index(CW_ARGS){
 CW_LOCAL;if(lane>=w[10]||w[CW_LEADER+lane]!=lane)return;device long*a=packets+lane*CW_STRIDE;if(a[2]>=0)return;
 uint row=w[5]+w[CW_BLOCK+3*(lane/256)]+w[CW_RP+lane],h=cw_hash(a)&(HASH_CAP-1);device atomic_uint*index=(device atomic_uint*)(r+INDEX_OFFSET+1);
 while(true){uint old=atomic_load_explicit(index+2*h,memory_order_relaxed);if(!old){uint expected=0;if(atomic_compare_exchange_weak_explicit(index+2*h,&expected,row+1,memory_order_relaxed,memory_order_relaxed)){uint at=atomic_fetch_add_explicit((device atomic_uint*)(r+RC_INDEX_TOUCH),1u,memory_order_relaxed);r[RC_INDEX_TOUCH+1+at]=h;return;}}else h=(h+1)&(HASH_CAP-1);}
}
kernel void cell_wave_resolve(CW_ARGS){
 CW_LOCAL;if(lane>=w[10])return;device long*a=packets+lane*CW_STRIDE;device long*o=data+uint(a[11])*BC_STRIDE;
 if(!w[0]){if(bc_waiting(int(a[10]),o))cw_add(stats,21);return;}
 if(a[0]==-1)return;
 long left=a[3],right=a[4];if(a[0]>=0){a[4]=packets[w[CW_LEADER+lane]*CW_STRIDE+4];left=a[4];right=a[3];
  if(stats[14]){uint t=uint(a[0]);long key[COLS];for(uint j=0;j<WIDTH[t]-1;j++)key[j]=a[12+j];long row=query_lookup(r,t,key);cw_add(stats,23);
   if(row<0||cw_canon(p,t,WIDTH[t]-1,r[row*STRIDE+4+WIDTH[t]-1])!=cw_canon(p,t,WIDTH[t]-1,a[4])){atomic_store_explicit((device atomic_uint*)&m.error,36u,memory_order_relaxed);return;}}
 }
 if(a[8]){uint at=w[8]+w[CW_BLOCK+3*(lane/256)+2]+w[CW_UP+lane];p[IDCAP+2+2*at]=uint(left);p[IDCAP+3+2*at]=uint(right);}
 if(a[1]==1){cw_add(stats,18);if(KIND[uint(a[0])]==0)cw_add(stats,20);}
 cw_reply(int(a[10]),o,a,stats);
}
// Completion tickets are filled in source-context order, then indexed in
// parallel. Hash placement is internal; record identities and payloads are stable.
kernel void cell_wave_cert_offsets(CW_ARGS){
 CW_LOCAL;if(lane||w[0]||p[IDCAP+1])return;uint n=0;
 for(uint tile=control[31];tile<control[32];tile++){w[CW_RP+tile-control[31]]=n;n+=uint(meta[3*tile+1])*bc_certificate_bound(int(meta[3*tile+2]));}
 uint old=uint(stats[15]);if(old+n>batchCfg.x){m.error=34;return;}w[20]=old;w[21]=n;stats[15]=old+n;
 if(n)atomic_fetch_max_explicit(dispatch+BC_WAVE_CERT,(max(n,w[10])+255)/256,memory_order_relaxed);
}
kernel void cell_wave_cert_write(CW_ARGS){
 CW_LOCAL;if(w[0]||lane>=w[10]||!w[21])return;uint ordinal=control[23]+lane,lo=control[31],hi=control[32];
 while(lo+1<hi){uint mid=(lo+hi)/2;if(uint(meta[3*mid])<=ordinal)lo=mid;else hi=mid;}
 int plan=int(meta[3*lo+2]);uint start=w[20]+w[CW_RP+lo-control[31]]+(ordinal-uint(meta[3*lo]))*bc_certificate_bound(plan);
 uint base=BC_STATS+batchCfg.x*(5+BC_STRIDE);cw_certificates(plan,data+ordinal*BC_STRIDE,r,p,stats,start,base);
}
kernel void cell_wave_cert_hash(CW_ARGS){
 CW_LOCAL;if(w[0]||lane>=w[21])return;uint cap=batchCfg.x,base=BC_STATS+cap*(3+BC_STRIDE),records=base+2*cap;
 uint record=w[20]+lane,at=records+4*record,fid=uint(stats[at+1]);if(!fid)return;
 uint rule=uint(stats[at]),rev=uint(stats[at+2]),h=bc_hash(rule,fid)&(2*cap-1);device atomic_uint*heads=(device atomic_uint*)(stats+base);
 while(true){uint old=atomic_load_explicit(heads+2*h,memory_order_relaxed);
  if(!old){uint expected=0;if(atomic_compare_exchange_weak_explicit(heads+2*h,&expected,record+1,memory_order_relaxed,memory_order_relaxed))return;}
  else{uint prev=records+4*(old-1);if(stats[prev]==rule&&stats[prev+1]==fid){
   if(uint(stats[prev+2])>rev||(uint(stats[prev+2])==rev&&old<=record+1))return;
   uint expected=old;if(atomic_compare_exchange_weak_explicit(heads+2*h,&expected,record+1,memory_order_relaxed,memory_order_relaxed))return;
  }else h=(h+1)&(2*cap-1);}
 }
}
kernel void cell_wave_finish(CW_ARGS){
 CW_LOCAL;if(lane)return;if(!w[0]){control[27]=control[24];stats[10]=10;}
}

// Equality requests commute. Drain them on-device, then canonicalize the entire
// active parent forest so scheduling cannot change the next Run's identity view.
#define CU_LOCAL \
 uint instance=point.y,lane=point.x;if(instance>=cfg.x)return; \
 device Meta&m=all_meta[instance];device long*r=all_rows+instance*GC_ROW_WORDS; \
 device uint*p=all_uint+instance*GC_UINT_WORDS; \
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL; \
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE)); \
 if(m.error||stats[10]!=12)return;
kernel void cell_union_gate(BC_ARGS,uint2 point[[thread_position_in_grid]]){
 CU_LOCAL;if(lane)return;
 atomic_fetch_max_explicit(dispatch+BC_WAVE_UNIONS,(p[IDCAP+1]+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_WAVE_IDS,(m.next_id+255)/256,memory_order_relaxed);
 atomic_fetch_max_explicit(dispatch+BC_WAVE_ONE,1u,memory_order_relaxed);cw_add(stats,50);cw_add(stats,53,p[IDCAP+1]);
}
kernel void cell_union_apply(BC_ARGS,uint2 point[[thread_position_in_grid]]){
 CU_LOCAL;if(lane>=p[IDCAP+1])return;
 uint a=p[IDCAP+2+2*lane],b=p[IDCAP+3+2*lane];device atomic_uint*parents=(device atomic_uint*)p;
 while(true){uint x=parallel_root(parents,a),y=parallel_root(parents,b);if(x==y)return;
  uint hi=max(x,y),lo=min(x,y),expected=hi;
  if(atomic_compare_exchange_weak_explicit(parents+hi,&expected,lo,memory_order_relaxed,memory_order_relaxed)){
   atomic_fetch_add_explicit(parents+MERGE_GENERATION,1u,memory_order_relaxed);
   atomic_fetch_add_explicit((device atomic_uint*)&m.unions,1u,memory_order_relaxed);
   atomic_store_explicit((device atomic_uint*)&m.changed,1u,memory_order_relaxed);return;
  }
 }
}
kernel void cell_union_flatten(BC_ARGS,uint2 point[[thread_position_in_grid]]){
 CU_LOCAL;if(lane>=m.next_id)return;device atomic_uint*parents=(device atomic_uint*)p;
 uint root=parallel_root(parents,lane);atomic_store_explicit(parents+lane,root,memory_order_relaxed);
 if(atomic_load_explicit(parents+MERGE_KEY_FLAGS+lane,memory_order_relaxed))atomic_store_explicit(parents+MERGE_KEY_FLAGS+root,1u,memory_order_relaxed);
}
kernel void cell_union_finish(BC_ARGS,uint2 point[[thread_position_in_grid]]){
 CU_LOCAL;if(lane)return;p[IDCAP+1]=0;bool changed=m.unions!=control[34];
 if(changed){stats[17]++;if(BC_PARALLEL_REBUILD){control[0]=1;control[21]=control[35];stats[10]=6;return;}
  device uint*fp=all_uint+instance*GC_UINT_WORDS+GC_FP;device uint*qheads=all_uint+instance*GC_UINT_WORDS+GC_QHEADS;
  device uint*qlinks=all_uint+instance*GC_UINT_WORDS+GC_QLINKS;rebuild(r,p,fp,m,qheads,qlinks);
 }
 bc_finish_run(BC_VALUES,instance,changed&&control[35]);
}
