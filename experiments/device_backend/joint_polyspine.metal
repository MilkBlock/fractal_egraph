// The global row view is cloned once for the entire nested Saturate. Only
// constructor records and a small affected-key index are written in that
// private view. Structural interfaces and unresolved ports remain resident.
// JOINT_PROGRAM
#define JP_ARGS device long*r[[buffer(0)]],device uint*p[[buffer(1)]],device uint*fp[[buffer(2)]],device Meta&m[[buffer(3)]],device uint*wm[[buffer(4)]],device const uint*vm[[buffer(29)]],device const uint*svmap[[buffer(28)]],device const long*svdata[[buffer(30)]]
uint jp_add(device long*r,uint field,uint n){return atomic_fetch_add_explicit((device atomic_uint*)(r+JP_STATE+field),n,memory_order_relaxed);}
void jp_fail(device Meta&m,uint e){atomic_store_explicit((device atomic_uint*)&m.error,e,memory_order_relaxed);}
uint jp_count(device long*r,uint field){return uint(r[JP_STATE+field]);}
void jp_intern(uint key,device uint*map,device uint*roots,device long*r){
 uint expected=0xffffffff;while(!atomic_compare_exchange_weak_explicit((device atomic_uint*)(map+key),&expected,0xfffffffeu,memory_order_relaxed,memory_order_relaxed)){if(expected!=0xffffffff)return;} {
  uint i=jp_add(r,12,1);roots[i]=key;atomic_store_explicit((device atomic_uint*)(map+key),i,memory_order_relaxed);}
}
kernel void jp_begin(JP_ARGS,uint i[[thread_position_in_grid]]){
 if(i)return;for(uint j=0;j<128;j++)r[JP_STATE+j]=0;
 r[JP_STATE]=1;r[JP_STATE+2]=m.count;r[JP_STATE+3]=m.epoch;r[JP_STATE+4]=m.rounds;r[JP_STATE+5]=m.unions;r[JP_STATE+19]=m.next_id;r[JP_STATE+23]=m.count;r[JP_STATE+24]=m.next_fact;r[JP_STATE+104]=r[ROW_WORDS];
}
kernel void jp_zero_maps(JP_ARGS,device uint*rootmap[[buffer(5)]],device uint*fmap[[buffer(6)]],device uint*props[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i<uint(r[JP_STATE+19])){rootmap[i]=0xffffffff;fmap[i]=0;props[i]=0;}
 if(i<IDCAP)r[JP_VERSION+i]=0;if(i<6)r[JP_STATE+32+i]=0;
 if(i<m.count)r[JP_ROW_TAG+i]=3;
}
kernel void jp_root_reset(JP_ARGS,device uint*rootstate[[buffer(5)]],device float*scores[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,12))return;for(uint j=0;j<8;j++){rootstate[8*i+j]=0;scores[8*i+j]=j<3?1.0f:0.0f;}
}
kernel void jp_count_range(JP_ARGS,constant uint2&range[[buffer(5)]],uint i[[thread_position_in_grid]]){
 i+=range.x;if(i>=range.y)return;uint t=uint(r[i*STRIDE]);if(r[i*STRIDE+1]==2)return;
 for(uint j=0;j<4;j++)if(t==JP_IDS[j]){if(r[i*STRIDE+1])jp_fail(m,20);jp_add(r,32+j,1);jp_add(r,36,1);}
 if(range.x==0&&r[i*STRIDE+1]==0&&jp_recipe(t)>=0)jp_add(r,37,1);
}
kernel void jp_pack_range(JP_ARGS,device uint*markers[[buffer(5)]],device uint*lists[[buffer(6)]],device uint*fmap[[buffer(7)]],device uint*rootmap[[buffer(8)]],device uint*roots[[buffer(9)]],device uint*props[[buffer(10)]],constant uint3&cfg[[buffer(11)]],uint i[[thread_position_in_grid]]){
 i+=cfg.x;if(i>=cfg.y||r[i*STRIDE+1])return;uint t=uint(r[i*STRIDE]),lc=cfg.z;
 for(uint j=0;j<4;j++)if(t==JP_IDS[j]){
  uint n=jp_add(r,13+j,1);lists[j*lc+n]=i;lists[4*lc+jp_add(r,17,1)]=i;
  if(j==1||j==2){uint key=uint(r[i*STRIDE+4]);if(key>=uint(r[JP_STATE+19])){jp_fail(m,21);return;}jp_intern(key,rootmap,roots,r);if(j==2)fmap[key]=i+1;}
 }
 if(cfg.x==0){
  int recipe=jp_recipe(t);if(recipe>=0){uint n=jp_add(r,11,1),q=8*n,arity=JP_ARITY[recipe],key=uint(r[i*STRIDE+4+jp_root_col(recipe)]);
   markers[q]=i;markers[q+1]=uint(recipe);markers[q+2]=key;markers[q+3]=arity;markers[q+7]=uint(r[i*STRIDE+3]);jp_intern(key,rootmap,roots,r);
   for(uint j=0;j<3;j++){uint child=j<arity?uint(r[i*STRIDE+4+jp_child(recipe,j)]):0;markers[q+4+j]=child;if(j<arity)jp_intern(child,rootmap,roots,r);}}
  for(uint j=0;j<3;j++)if(t==JP_PROPERTIES[j])atomic_fetch_or_explicit((device atomic_uint*)(props+uint(r[i*STRIDE+4])),1u<<j,memory_order_relaxed);
 }
}
// Affected-key congruence is maintained locally. Every other table's keys
// are proven immutable in this scope, so its original complete-key index
// remains usable without global rebuild after each helper iteration.
kernel void jp_hash_clear(JP_ARGS,uint i[[thread_position_in_grid]]){if(i<HASH_CAP)r[JP_HASH+i]=0;}
kernel void jp_hash_insert(JP_ARGS,device const uint*lists[[buffer(5)]],constant uint&lc[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,17))return;uint row=lists[4*lc+i],t=uint(r[row*STRIDE]);if(r[row*STRIDE+1]||!jp_key_table(t))return;
 long args[COLS]={0};for(uint j=0;j<WIDTH[t]-1;j++)args[j]=r[row*STRIDE+4+j];uint h=row_hash(t,args,r,p);
 device atomic_uint*heads=(device atomic_uint*)(r+JP_HASH);
 while(true){uint old=atomic_load_explicit(heads+2*h,memory_order_relaxed);
  if(!old){uint expected=0;if(atomic_compare_exchange_weak_explicit(heads+2*h,&expected,row+1,memory_order_relaxed,memory_order_relaxed))return;}
  else if(keyeq(r,old-1,t,args,p)){atomic_fetch_min_explicit(heads+2*h,row+1,memory_order_relaxed);return;}
  else h=(h+1)&(HASH_CAP-1);}
}
kernel void jp_canonical(JP_ARGS,device const uint*lists[[buffer(5)]],constant uint&lc[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,17))return;uint row=lists[4*lc+i],t=uint(r[row*STRIDE]);if(r[row*STRIDE+1]==2)return;
 for(uint j=0;j<WIDTH[t];j++)if(EQ[t*COLS+j]){uint at=row*STRIDE+4+j;long v=long(cu_root(p,uint(r[at])));if(v!=r[at]){sc_dirty(r,row,m.epoch);r[at]=v;r[row*STRIDE+2]=m.epoch;r[JP_ROW_TAG+row]=2;}}
}
kernel void jp_retire(JP_ARGS,device const uint*lists[[buffer(5)]],constant uint&lc[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,17))return;uint row=lists[4*lc+i],t=uint(r[row*STRIDE]);if(r[row*STRIDE+1]||!jp_key_table(t))return;
 long args[COLS]={0};for(uint j=0;j<WIDTH[t]-1;j++)args[j]=r[row*STRIDE+4+j];uint h=row_hash(t,args,r,p);device long*heads=r+JP_HASH;
 while(heads[h]&&!keyeq(r,uint(heads[h]-1),t,args,p))h=(h+1)&(HASH_CAP-1);
 if(!heads[h]){jp_fail(m,22);return;}uint leader=uint(heads[h]-1);if(leader==row)return;
 parallel_unite(p,r[leader*STRIDE+4+WIDTH[t]-1],r[row*STRIDE+4+WIDTH[t]-1],m);
 device atomic_uint*f=(device atomic_uint*)fp;uint a=uint(r[row*STRIDE+3]),b=uint(r[leader*STRIDE+3]);
 while(true){a=parallel_root(f,a);b=parallel_root(f,b);if(a==b)break;uint hi=max(a,b),lo=min(a,b),expected=hi;if(atomic_compare_exchange_weak_explicit(f+hi,&expected,lo,memory_order_relaxed,memory_order_relaxed))break;}
 sc_dirty(r,row,m.epoch);sc_dirty(r,leader,m.epoch);r[row*STRIDE+1]=2;r[leader*STRIDE+2]=m.epoch;r[JP_ROW_TAG+leader]=2;jp_add(r,23,0xffffffff);jp_add(r,47,1);
}
// Dense function-key map + CSR of complete cost witnesses. The payload row
// index identifies (Term,cost,return) together, not independent marginals.
kernel void jp_csr_clear(JP_ARGS,device uint*counts[[buffer(5)]],device uint*stamps[[buffer(6)]],device uint*ranges[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(!i)jp_add(r,106,1);if(i<m.next_id){counts[i]=0;stamps[i]=0;ranges[2*i]=0;ranges[2*i+1]=0;}
}
kernel void jp_csr_count(JP_ARGS,device const uint*lists[[buffer(5)]],device uint*counts[[buffer(6)]],device uint*stamps[[buffer(7)]],device uint*ranges[[buffer(8)]],constant uint&lc[[buffer(9)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,13))return;uint row=lists[i];if(sv[row*STRIDE+1])return;uint id=uint(sv[row*STRIDE+6]);
 atomic_fetch_add_explicit((device atomic_uint*)(counts+id),1u,memory_order_relaxed);atomic_fetch_max_explicit((device atomic_uint*)(stamps+id),uint(sv[row*STRIDE+2]),memory_order_relaxed);
 cu_extreme((device atomic_uint*)(ranges+2*id),row,sv,false);cu_extreme((device atomic_uint*)(ranges+2*id+1),row,sv,true);
}
kernel void jp_csr_offsets(JP_ARGS,device const uint*counts[[buffer(5)]],device uint*offsets[[buffer(6)]],device uint*cursors[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i)return;uint n=0;for(uint j=0;j<m.next_id;j++){offsets[j]=n;cursors[j]=0;n+=counts[j];}offsets[m.next_id]=n;
}
kernel void jp_csr_scan(JP_ARGS,device const uint*counts[[buffer(5)]],device uint*offsets[[buffer(6)]],device uint*blocks[[buffer(7)]],uint i[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 threadgroup uint sum[256];uint n=i<m.next_id?counts[i]:0;sum[tid]=n;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sum[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sum[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(i<m.next_id)offsets[i]=sum[tid]-n;if(tid==255)blocks[group]=sum[tid];
}
kernel void jp_csr_blocks(JP_ARGS,device uint*blocks[[buffer(5)]],device uint*offsets[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i)return;uint n=0;for(uint j=0;j<(m.next_id+255)/256;j++){uint v=blocks[j];blocks[j]=n;n+=v;}offsets[m.next_id]=n;
}
kernel void jp_csr_add(JP_ARGS,device uint*offsets[[buffer(5)]],device uint*cursors[[buffer(6)]],device const uint*blocks[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i<m.next_id){offsets[i]+=blocks[i/256];cursors[i]=0;}
}
kernel void jp_csr_fill(JP_ARGS,device const uint*lists[[buffer(5)]],device const uint*offsets[[buffer(6)]],device uint*cursors[[buffer(7)]],device uint*items[[buffer(8)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,13))return;uint row=lists[i];if(sv[row*STRIDE+1])return;uint id=uint(sv[row*STRIDE+6]);uint at=atomic_fetch_add_explicit((device atomic_uint*)(cursors+id),1u,memory_order_relaxed);items[offsets[id]+at]=row;
}
kernel void jp_prepare(JP_ARGS,uint i[[thread_position_in_grid]]){
 if(i)return;if(m.epoch==0xffffffff){m.error=6;return;}m.epoch++;m.changed=0;m.matched=0;r[ROW_WORDS+1]=m.epoch;
 r[JP_STATE+20]=m.count;r[JP_STATE+22]=r[JP_STATE+23];r[QUERY_LOG+1]=m.count;r[JP_STATE+9]=0;r[JP_STATE+21]++;
}
kernel void jp_groups(JP_ARGS,device const uint*markers[[buffer(5)]],device const uint*lists[[buffer(6)]],device const uint*fmap[[buffer(7)]],device const uint*counts[[buffer(8)]],device const uint*offsets[[buffer(9)]],device const uint*stamps[[buffer(10)]],device uint*g[[buffer(11)]],constant uint2&cfg[[buffer(12)]],uint i[[thread_position_in_grid]]){
 if(i>=cfg.x||m.error)return;uint q=16*i;for(uint j=0;j<16;j++)g[q+j]=0;
 uint phase=jp_count(r,1),row,rule,recipe,root,arity=0,refs[3]={0},sizes[3]={1,1,1},starts[3]={0},latest=0;
 if(phase==0){uint b=8*i;row=markers[b];recipe=markers[b+1];root=markers[b+2];arity=markers[b+3];rule=JP_TERM_RULES[recipe];latest=uint(sv[row*STRIDE+2]);
  for(uint j=0;j<arity;j++){uint fr=fmap[markers[b+4+j]];if(!fr)return;refs[j]=fr-1;uint id=uint(sv[(fr-1)*STRIDE+5]);sizes[j]=counts[id];starts[j]=offsets[id];latest=max(latest,max(uint(sv[(fr-1)*STRIDE+2]),stamps[id]));}
 }else{uint rank=i%3;row=lists[cfg.y+i/3];if(sv[row*STRIDE+1])return;root=uint(sv[row*STRIDE+4]);recipe=rank;rule=JP_HELPER_RULES[rank];latest=uint(sv[row*STRIDE+2]);
  if(rank){uint id=uint(sv[row*STRIDE+5]);sizes[0]=counts[id];starts[0]=offsets[id];latest=max(latest,stamps[id]);arity=1;}
  if(rank==1){uint fr=fmap[root];if(!fr)return;refs[0]=fr-1;uint id=uint(sv[(fr-1)*STRIDE+5]);sizes[1]=counts[id];starts[1]=offsets[id];latest=max(latest,max(uint(sv[(fr-1)*STRIDE+2]),stamps[id]));arity=2;}
 }
 if(jp_seminaive(rule)&&latest<=wm[rule])return;ulong n=ulong(sizes[0])*sizes[1]*sizes[2];if(n>MATCHCAP){jp_fail(m,2);return;}
 g[q]=uint(n);g[q+2]=row;g[q+3]=rule;g[q+4]=recipe;g[q+5]=root;
 for(uint j=0;j<3;j++){g[q+6+j]=refs[j];g[q+9+j]=sizes[j];g[q+12+j]=starts[j];}
}
kernel void jp_group_offsets(JP_ARGS,device uint*g[[buffer(5)]],constant uint&groups[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i||m.error)return;ulong n=0;for(uint j=0;j<groups;j++){g[16*j+1]=uint(n);n+=g[16*j];if(n>MATCHCAP){m.error=2;return;}}r[JP_STATE+9]=long(n);
}
kernel void jp_group_scan(JP_ARGS,device uint*g[[buffer(5)]],device uint*blocks[[buffer(6)]],constant uint&groups[[buffer(7)]],uint i[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]],uint block[[threadgroup_position_in_grid]]){
 threadgroup uint sum[256];uint n=i<groups?g[16*i]:0;sum[tid]=min(n,uint(MATCHCAP)+1);threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sum[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sum[tid]=uint(min(ulong(MATCHCAP)+1,ulong(sum[tid])+v));threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(i<groups)g[16*i+1]=sum[tid]>=n?sum[tid]-n:0;if(tid==255){blocks[block]=sum[tid];if(sum[tid]>MATCHCAP)jp_fail(m,2);}
}
kernel void jp_group_blocks(JP_ARGS,device uint*blocks[[buffer(5)]],constant uint&groups[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i||m.error)return;ulong n=0;for(uint j=0;j<(groups+255)/256;j++){uint v=blocks[j];blocks[j]=uint(n);n+=v;if(n>MATCHCAP){m.error=2;return;}}r[JP_STATE+9]=long(n);
}
kernel void jp_group_add(JP_ARGS,device uint*g[[buffer(5)]],device const uint*blocks[[buffer(6)]],constant uint&groups[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i<groups&&!m.error)g[16*i+1]+=blocks[i/256];
}
kernel void jp_bindings(JP_ARGS,device const uint*g[[buffer(5)]],device const uint*items[[buffer(6)]],device long*matches[[buffer(7)]],device uint*witnesses[[buffer(8)]],device uint*valid[[buffer(9)]],device uint*jobinfo[[buffer(10)]],device half*x[[buffer(11)]],device const uint*rootmap[[buffer(12)]],device const float*scores[[buffer(13)]],constant uint4&cfg[[buffer(14)]],uint job[[thread_position_in_grid]]){
 if(job>=jp_count(r,9)||m.error)return;uint lo=0,hi=cfg.x;while(lo+1<hi){uint mid=(lo+hi)/2;if(g[mid*16+1]<=job)lo=mid;else hi=mid;}uint at=lo*16;
 uint rootid=g[at+5],rootIndex=rootmap[rootid],phase=jp_count(r,1);bool priority=cfg.z==0||scores[rootIndex*8+phase]>=.5f;if(priority!=(cfg.w==1))return;
 valid[job]=0;for(uint j=0;j<cfg.y;j++)x[job*cfg.y+j]=0;uint rule=g[at+3],recipe=g[at+4],offset=job-g[at+1],selected[7];for(uint j=0;j<7;j++)selected[j]=0xffffffff;selected[0]=g[at+2];uint costs[3]={0xffffffff,0xffffffff,0xffffffff};
 for(uint j=0;j<3;j++){uint n=g[at+9+j],digit=n?offset%n:0;offset=n?offset/n:0;if((phase==0&&j<JP_ARITY[recipe])||(phase==1&&((recipe==1&&j<2)||(recipe==2&&j==0))))costs[j]=items[g[at+12+j]+digit];}
 if(phase==0){for(uint j=0;j<JP_ARITY[recipe];j++){selected[1+j]=g[at+6+j];selected[4+j]=costs[j];}}
 else{selected[1]=g[at+6];selected[2]=costs[0];selected[3]=costs[1];}
 long v[VARS]={0};uint w[JP_ATOMS];for(uint j=0;j<JP_ATOMS;j++)w[j]=0xffffffff;jp_add(r,48+phase,1);
 if(!jp_bind(rule,selected,sv,vm[0],v,w))return;uint atoms=jp_atom_count(rule),pivot=0;bool fresh=!jp_seminaive(rule);
 if(jp_seminaive(rule)){for(uint j=0;j<atoms;j++)if(uint(sv[w[j]*STRIDE+2])>wm[rule]){pivot=jp_delta(rule)?j:0;fresh=true;break;}}if(!fresh)return;
 for(uint j=0;j<atoms;j++){uint tag=uint(r[JP_ROW_TAG+w[j]]),producer=tag&3;if(producer<3&&producer!=phase){jp_add(r,59,1);uint bit=1u<<(8+phase);uint old=atomic_fetch_or_explicit((device atomic_uint*)(r+JP_ROW_TAG+w[j]),bit,memory_order_relaxed);if(!(old&bit))jp_add(r,60,1);}}
 uint q=job*MSTRIDE;matches[q]=rule;for(uint j=0;j<VARS;j++)matches[q+1+j]=v[j];for(uint j=0;j<JP_ATOMS;j++)witnesses[job*JP_ATOMS+j]=w[j];
 uint seed=jp_seed_atom(rule,pivot);matches[q+1+VARS]=jp_rule_rank(rule);matches[q+2+VARS]=pivot;matches[q+3+VARS]=jp_delta(rule)&&seed!=0xffffffff&&wm[rule]&&seed==pivot?uint(sv[w[seed]*STRIDE+2]):0;matches[q+4+VARS]=seed==0xffffffff?0:w[seed];
 for(uint j=5+VARS;j<MSTRIDE;j++)matches[q+j]=0;
 jobinfo[job*6]=rootIndex;jobinfo[job*6+1]=recipe;bool safe=phase==0;
 for(uint j=0;j<3;j++){jobinfo[job*6+2+j]=costs[j];if(costs[j]!=0xffffffff){long value=sv[costs[j]*STRIDE+5];safe=safe&&value>=-2048&&value<=2048;}}
 jobinfo[job*6+5]=safe;
 for(uint j=0;j<cfg.y;j++)x[job*cfg.y+j]=0;
 if(safe){for(uint j=0;j<3;j++)if(costs[j]!=0xffffffff)x[job*cfg.y+4*recipe+j]=half(sv[costs[j]*STRIDE+5]);x[job*cfg.y+4*recipe+3]=1;}
 valid[job]=1;
}
kernel void jp_affine_mma(JP_ARGS,device const half*x[[buffer(5)]],device const half*w[[buffer(6)]],device float*y[[buffer(7)]],constant uint&width[[buffer(8)]],uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 if(m.error)return;uint row=tile*8;threadgroup half a[64],b[64];threadgroup float out[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<width;k+=8){for(uint q=lane;q<64;q+=32){uint rr=q/8,cc=q%8;a[q]=row+rr<jp_count(r,9)?x[(row+rr)*width+k+cc]:half(0);b[q]=w[(k+rr)*8+cc];}threadgroup_barrier(mem_flags::mem_threadgroup);
 simdgroup_half8x8 aa,bb;simdgroup_load(aa,a,8);simdgroup_load(bb,b,8);simdgroup_multiply_accumulate(acc,aa,bb,acc);threadgroup_barrier(mem_flags::mem_threadgroup);}
 simdgroup_store(acc,out,8);threadgroup_barrier(mem_flags::mem_threadgroup);for(uint q=lane;q<64;q+=32)if(row+q/8<jp_count(r,9))y[(row+q/8)*8+q%8]=out[q];
}
kernel void jp_validate(JP_ARGS,device const uint*info[[buffer(5)]],device const float*y[[buffer(6)]],device uint*valid[[buffer(7)]],device long*numeric[[buffer(8)]],device uint*order[[buffer(9)]],uint job[[thread_position_in_grid]]){
 if(job>=jp_count(r,9)||!valid[job]||m.error)return;
 if(jp_count(r,1)==0){long values[JP_OPS]={0},v[3+JP_OPS]={0},cost=0;for(uint j=0;j<3;j++){uint row=info[6*job+2+j];if(row!=0xffffffff)v[j]=sv[row*STRIDE+5];}
  if(!jp_evaluate(info[6*job+1],v,values,cost)){jp_fail(m,5);return;}
  if(info[6*job+5]){if(long(y[job*8])!=cost){jp_fail(m,23);return;}int op=jp_result_op(info[6*job+1]);if(op>=0){values[op]=long(y[job*8]);jp_add(r,54,1);}}else jp_add(r,55,1);
  for(uint j=0;j<JP_OPS;j++)numeric[job*JP_OPS+j]=values[j];
 }
 uint at=atomic_fetch_add_explicit((device atomic_uint*)&m.matched,1u,memory_order_relaxed);order[at]=job;
}
bool jp_before(uint a,uint b,device long*matches,device const uint*witnesses){
 if(a==0xffffffff)return false;if(b==0xffffffff)return true;
 uint qa=a*MSTRIDE,qb=b*MSTRIDE;for(uint j=1+VARS;j<=4+VARS;j++){long x=matches[qa+j],y=matches[qb+j];if(x!=y)return x<y;}
 uint rule=uint(matches[qa]),pivot=uint(matches[qa+2+VARS]),n=jp_atom_count(rule);
 for(uint j=0;j<n;j++){uint atom=jp_order_atom(rule,pivot,j),x=witnesses[a*JP_ATOMS+atom],y=witnesses[b*JP_ATOMS+atom];if(x!=y)return x<y;}return a<b;
}
kernel void jp_sort_local(JP_ARGS,device long*matches[[buffer(5)]],device const uint*witnesses[[buffer(6)]],device uint*order[[buffer(7)]],uint lane[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 threadgroup uint data[256];data[tid]=lane<m.matched?order[lane]:0xffffffff;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint k=2;k<=256;k*=2)for(uint d=k/2;d;d/=2){uint j=tid^d,a=data[tid],b=data[j];bool low=((lane&k)==0)==(tid<j),before=jp_before(a,b,matches,witnesses);threadgroup_barrier(mem_flags::mem_threadgroup);data[tid]=low?(before?a:b):(before?b:a);threadgroup_barrier(mem_flags::mem_threadgroup);}order[lane]=data[tid];
}
kernel void jp_sort_global(JP_ARGS,device long*matches[[buffer(5)]],device const uint*witnesses[[buffer(6)]],device uint*order[[buffer(7)]],constant uint2&stage[[buffer(8)]],uint i[[thread_position_in_grid]]){
 uint j=i^stage.y;if(j<=i)return;uint a=order[i],b=order[j];if(jp_before(a,b,matches,witnesses)!=((i&stage.x)==0)){order[i]=b;order[j]=a;}
}
kernel void jp_commit(JP_ARGS,device long*matches[[buffer(5)]],device const uint*order[[buffer(6)]],device const long*numeric[[buffer(7)]],device const uint*info[[buffer(8)]],device long*code[[buffer(9)]],device long*registers[[buffer(10)]],device uint*rootstate[[buffer(11)]],device const uint*roots[[buffer(12)]],uint lane[[thread_position_in_grid]]){
 if(lane||m.error)return;p[IDCAP]=1;p[IDCAP+1]=0;
 uint phase=jp_count(r,1);
 for(uint i=0;i<m.matched&&!m.error;i++){uint job=order[i],rule=uint(matches[job*MSTRIDE]),owner=info[job*6],before=m.count;
  long key[COLS]={long(roots[owner])};long oldrow=phase==1?query_lookup(r,JP_IDS[2],key):-1,oldvalue=oldrow>=0?r[oldrow*STRIDE+5]:-1;
  if(phase==0)for(uint j=0;j<JP_OPS;j++)r[JP_NUM+j]=numeric[job*JP_OPS+j];
  execute_bytecode(rule,job,code,registers,matches,r,p,fp,m);rootstate[owner*8]|=1u<<phase;
  rootstate[owner*8+1+phase]+=m.count-before;jp_add(r,56+phase,m.count-before);
  if(phase==1){long nextrow=query_lookup(r,JP_IDS[2],key),nextvalue=nextrow>=0?r[nextrow*STRIDE+5]:-1;if(nextvalue!=oldvalue){rootstate[owner*8+4]++;jp_add(r,61,1);}}
 }
 p[IDCAP]=0;if(p[IDCAP+1]){m.error=24;return;}
 r[JP_STATE+23]+=m.count-uint(r[JP_STATE+20]);uint set=JP_SETS[phase];for(uint i=SET_START[set];i<SET_START[set+1];i++)wm[SET_RULES[i]]=m.epoch-1;
}
kernel void jp_comparison_roots(JP_ARGS,device const uint*roots[[buffer(5)]],device const uint*fmap[[buffer(6)]],device uint*rootstate[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,12))return;uint f=fmap[roots[i]];if(!f)return;uint id=uint(r[(f-1)*STRIDE+5]);
 if(uint(r[JP_VERSION+id])==m.epoch){rootstate[i*8]|=4;rootstate[i*8+3]++;}
}
kernel void jp_metrics(JP_ARGS,device const uint*rootstate[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(i==0){r[JP_STATE+98]=r[JP_STATE+96]-r[JP_STATE+97];r[JP_STATE+100]=r[JP_STATE+96]-r[JP_STATE+99];}
 if(i<jp_count(r,12))jp_add(r,80+(rootstate[8*i]&7),1);
 if(i<m.count){uint fid=uint(r[i*STRIDE+3]);if(fid>=jp_count(r,24))jp_add(r,88,1);else{jp_add(r,90,1);if(uint(r[i*STRIDE+2])>jp_count(r,3))jp_add(r,89,1);}}
}
kernel void jp_advance(JP_ARGS,uint i[[thread_position_in_grid]]){
 if(i||m.error)return;uint phase=jp_count(r,1);r[JP_STATE+6]|=m.changed;
 if(phase==0){r[JP_STATE+7]=m.changed;r[JP_STATE+1]=1;}
 else if(phase==1){r[JP_STATE+8]=m.changed;r[JP_STATE+1]=2;}
 else{bool inner=r[JP_STATE+8]||m.changed;r[JP_STATE+7]|=inner;r[JP_STATE+1]=inner?1:(r[JP_STATE+7]?0:3);}
}
kernel void jp_close(JP_ARGS,uint i[[thread_position_in_grid]]){
 if(i)return;m.changed=uint(r[JP_STATE+6]);m.rounds=uint(r[JP_STATE+4])+1;r[JP_STATE]=0;r[INDEX_OFFSET]=0;r[QUERY_DIRTY]=1;
}
// Typed, ID-invariant Polyspine features share the same exact interfaces.
// Pooled values are prediction inputs only; jp_bind still retains every tuple.
kernel void jp_static_markers(JP_ARGS,device const uint*markers[[buffer(5)]],device const uint*rootmap[[buffer(6)]],device uint*counts[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,11))return;uint q=8*i,owner=rootmap[markers[q+2]],arity=markers[q+3],recipe=markers[q+1];device atomic_uint*base=(device atomic_uint*)(counts+owner*JP_FEATURES);
 atomic_fetch_add_explicit(base+10,1u,memory_order_relaxed);atomic_fetch_add_explicit(base+32+recipe,1u,memory_order_relaxed);
 for(uint j=0;j<arity;j++){uint child=rootmap[markers[q+4+j]];atomic_fetch_add_explicit((device atomic_uint*)(counts+child*JP_FEATURES+12),1u,memory_order_relaxed);
  for(uint k=0;k<j;k++)if(markers[q+4+j]==markers[q+4+k])atomic_fetch_add_explicit(base+15,1u,memory_order_relaxed);}
 uint row=markers[q];for(uint j=0;j<WIDTH[uint(r[row*STRIDE])];j++)if(jp_numeric_anchor(recipe,j)){long v=r[row*STRIDE+4+j];atomic_fetch_add_explicit(base+(v==0?29:(v>0?30:31)),1u,memory_order_relaxed);}
}
kernel void jp_static_heads(JP_ARGS,device const uint*rootmap[[buffer(5)]],device const half*codes[[buffer(6)]],device int*heads[[buffer(7)]],device uint*counts[[buffer(8)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,2)||r[i*STRIDE+1])return;uint t=uint(r[i*STRIDE]);if(!JP_HEAD_TABLE[t])return;uint id=uint(r[i*STRIDE+4+WIDTH[t]-1]);if(id>=jp_count(r,19)||rootmap[id]==0xffffffff)return;
 uint owner=rootmap[id];atomic_fetch_add_explicit((device atomic_uint*)(counts+owner),1u,memory_order_relaxed);
 for(uint j=0;j<64;j++)atomic_fetch_add_explicit((device atomic_int*)(heads+owner*64+j),int(codes[t*64+j]),memory_order_relaxed);
}
kernel void jp_marker_offsets(JP_ARGS,device const uint*counts[[buffer(5)]],device uint*offsets[[buffer(6)]],device uint*cursor[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i)return;uint n=0;for(uint j=0;j<jp_count(r,12);j++){offsets[j]=n;cursor[j]=0;n+=counts[j*JP_FEATURES+10];}offsets[jp_count(r,12)]=n;
}
kernel void jp_marker_items(JP_ARGS,device const uint*markers[[buffer(5)]],device const uint*rootmap[[buffer(6)]],device const uint*offsets[[buffer(7)]],device uint*cursor[[buffer(8)]],device uint*items[[buffer(9)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,11))return;uint owner=rootmap[markers[i*8+2]];uint k=atomic_fetch_add_explicit((device atomic_uint*)(cursor+owner),1u,memory_order_relaxed);items[offsets[owner]+k]=i;
}
kernel void jp_feature_clear(JP_ARGS,device uint*summary[[buffer(5)]],device uint*choicecounts[[buffer(6)]],device uint*priority[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i==0)r[JP_STATE+96]+=jp_count(r,12);
 if(i<jp_count(r,12))for(uint j=0;j<4;j++)summary[i*4+j]=0;
 if(i<m.next_id){choicecounts[i]=0;priority[i]=0;}
}
kernel void jp_candidate_summary(JP_ARGS,device const uint*lists[[buffer(5)]],device const uint*rootmap[[buffer(6)]],device const uint*counts[[buffer(7)]],device const uint*ranges[[buffer(8)]],device uint*summary[[buffer(9)]],constant uint&lc[[buffer(10)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,14))return;uint row=lists[lc+i];if(r[row*STRIDE+1])return;uint owner=rootmap[uint(r[row*STRIDE+4])],cost=uint(r[row*STRIDE+5]);device atomic_uint*base=(device atomic_uint*)(summary+owner*4);
 atomic_fetch_add_explicit(base,1u,memory_order_relaxed);atomic_fetch_add_explicit(base+1,counts[cost],memory_order_relaxed);
 if(ranges[2*cost]){cu_extreme(base+2,ranges[2*cost]-1,r,false);cu_extreme(base+3,ranges[2*cost+1]-1,r,true);}
}
kernel void jp_choice_summary(JP_ARGS,device const uint*lists[[buffer(5)]],device uint*counts[[buffer(6)]],constant uint&lc[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,16))return;uint row=lists[3*lc+i];if(r[row*STRIDE+1])return;atomic_fetch_add_explicit((device atomic_uint*)(counts+uint(r[row*STRIDE+6])),1u,memory_order_relaxed);
}
float jp_log_count(uint x){return min(1.0f,log2(1.0f+float(x))/16.0f);}
float jp_magnitude(long x){ulong u=x<0?0UL-as_type<ulong>(x):ulong(x);return float((x>0)-(x<0))*float(64-clz(u))/64.0f;}
kernel void jp_features(JP_ARGS,device const uint*roots[[buffer(5)]],device const uint*fmap[[buffer(6)]],device const uint*costcount[[buffer(7)]],device const uint*ranges[[buffer(8)]],device const uint*summary[[buffer(9)]],device const uint*choices[[buffer(10)]],device const uint*props[[buffer(11)]],device const uint*markers[[buffer(12)]],device const uint*rootmap[[buffer(13)]],device const uint*moffset[[buffer(14)]],device const uint*mitems[[buffer(15)]],device const uint*stat[[buffer(16)]],device const uint*rootstate[[buffer(17)]],device long*initial[[buffer(18)]],device half*x[[buffer(19)]],constant uint&first[[buffer(20)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,12))return;float v[JP_FEATURES]={0};uint key=roots[i],fr=fmap[key],id=fr?uint(r[(fr-1)*STRIDE+5]):0;bool ready=fr&&costcount[id]>0;
 long low=ready?r[(ranges[2*id]-1)*STRIDE+5]:0,high=ready?r[(ranges[2*id+1]-1)*STRIDE+5]:0;
 if(first){initial[i*4]=ready;initial[i*4+1]=low;initial[i*4+2]=id;initial[i*4+3]=fr!=0;}
 v[0]=1;v[1+jp_count(r,1)]=1;v[4]=ready;v[5]=ready?jp_magnitude(low):0;v[6]=ready?jp_magnitude(high):0;v[7]=ready?jp_log_count(costcount[id]):0;v[8]=ready&&!initial[i*4];
 v[9]=jp_log_count(rootstate[i*8+1]+rootstate[i*8+2]+rootstate[i*8+3]+rootstate[i*8+4]);v[10]=jp_log_count(stat[i*JP_FEATURES+10]);v[12]=jp_log_count(stat[i*JP_FEATURES+12]);v[15]=jp_log_count(stat[i*JP_FEATURES+15]);
 v[16]=bool(props[key]&1);v[17]=bool(props[key]&2);v[18]=bool(props[key]&4);
 uint ports=0,nready=0,producer=0,pr[3]={0,0,0};
 for(uint q=moffset[i];q<moffset[i+1];q++){uint mi=mitems[q],b=8*mi,n=markers[b+3];for(uint j=0;j<n;j++){
  uint child=markers[b+4+j],cf=fmap[child];ports++;nready+=cf&&costcount[uint(r[(cf-1)*STRIDE+5])]>0;producer+=stat[rootmap[child]*JP_FEATURES+10];
  for(uint k=0;k<3;k++)pr[k]+=bool(props[child]&(1u<<k));}}
 v[11]=jp_log_count(producer);v[13]=float(nready)/max(1u,ports);v[14]=float(ports-nready)/max(1u,ports);for(uint k=0;k<3;k++)v[19+k]=float(pr[k])/max(1u,ports);
 v[22]=jp_log_count(summary[i*4]);v[23]=jp_log_count(summary[i*4+1]);v[24]=fr?jp_log_count(choices[id]):0;
 bool hasCandidate=summary[i*4+2]!=0;long cmin=hasCandidate?r[(summary[i*4+2]-1)*STRIDE+5]:0,cmax=hasCandidate?r[(summary[i*4+3]-1)*STRIDE+5]:0;
 v[25]=ready&&hasCandidate&&cmin<high;v[26]=ready&&hasCandidate&&cmax>=low;
 uint previous=jp_count(r,1)==2?m.epoch:m.epoch-1;v[27]=fr&&(uint(r[(fr-1)*STRIDE+2])==previous||uint(r[JP_VERSION+id])==previous);
 v[28]=jp_log_count(rootstate[i*8+1]+rootstate[i*8+2]);for(uint k=29;k<32;k++)v[k]=jp_log_count(stat[i*JP_FEATURES+k]);
 for(uint j=0;j<JP_RECIPES;j++)v[32+j]=float(stat[i*JP_FEATURES+32+j])/max(1u,stat[i*JP_FEATURES+10]);
 long numbers[4]={low,high,cmin,cmax};for(uint a=0;a<4;a++){ulong bits=(as_type<ulong>(numbers[a])^0x8000000000000000UL);for(uint b=0;b<8;b++)v[32+JP_RECIPES+8*a+b]=(a<2?ready:hasCandidate)?float((bits>>(56-8*b))&255)/255.0f:0;}
 for(uint j=0;j<JP_FEATURES;j++)x[i*JP_FEATURES+j]=half(v[j]);
}
kernel void jp_feature_mma(JP_ARGS,device const half*x[[buffer(5)]],device const half*e[[buffer(6)]],device float*y[[buffer(7)]],device const uint*dirty[[buffer(8)]],constant uint&cached[[buffer(9)]],uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 uint row=(tile/8)*8,col=(tile%8)*8;uint n=jp_count(r,12),base=cached?jp_count(r,1)*n:0;
 if(cached){bool active=false;for(uint j=row;j<min(row+8,n);j++)active|=dirty[j]!=0;if(!active)return;if(lane==0&&col==0)jp_add(r,101,min(8u,n-row));}threadgroup half a[64],b[64];threadgroup float out[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<JP_FEATURES;k+=8){for(uint q=lane;q<64;q+=32){uint rr=q/8,cc=q%8;a[q]=row+rr<jp_count(r,12)?x[(base+row+rr)*JP_FEATURES+k+cc]:half(0);b[q]=e[(k+rr)*64+col+cc];}threadgroup_barrier(mem_flags::mem_threadgroup);simdgroup_half8x8 aa,bb;simdgroup_load(aa,a,8);simdgroup_load(bb,b,8);simdgroup_multiply_accumulate(acc,aa,bb,acc);threadgroup_barrier(mem_flags::mem_threadgroup);}
 simdgroup_store(acc,out,8);threadgroup_barrier(mem_flags::mem_threadgroup);for(uint q=lane;q<64;q+=32)if(row+q/8<jp_count(r,12))y[(base+row+q/8)*64+col+q%8]=out[q]/sqrt(float(JP_FEATURES));
}
kernel void jp_seams(JP_ARGS,device const float*encoded[[buffer(5)]],device const int*heads[[buffer(6)]],device const uint*hcounts[[buffer(7)]],device const uint*markers[[buffer(8)]],device const uint*rootmap[[buffer(9)]],device const uint*offsets[[buffer(10)]],device const uint*items[[buffer(11)]],device const half*roles[[buffer(12)]],device half*x[[buffer(13)]],device const uint*dirty[[buffer(14)]],constant uint&cached[[buffer(15)]],uint i[[thread_position_in_grid]]){
 uint n=jp_count(r,12),base=cached?jp_count(r,1)*n:0;if(i>=n||(cached&&!dirty[i]))return;if(cached)jp_add(r,99,1);for(uint k=0;k<64;k++){float seam=float(heads[i*64+k])/max(1u,hcounts[i]),count=1;
  for(uint q=offsets[i];q<offsets[i+1];q++){uint mi=items[q],b=8*mi,rid=markers[b+1];for(uint j=0;j<markers[b+3];j++){uint child=rootmap[markers[b+4+j]],role=3*rid+j,at=(k+JP_ROLE_ROTATION[role])%64;
   seam+=float(roles[role*64+k])*(encoded[(base+child)*64+at]+float(heads[child*64+at])/max(1u,hcounts[child]));count++;}}
  x[(base+i)*136+k]=half(encoded[(base+i)*64+k]);x[(base+i)*136+64+k]=half(seam/count);}
 x[(base+i)*136+128]=1;for(uint k=129;k<136;k++)x[(base+i)*136+k]=0;
}
kernel void jp_ridge_mma(JP_ARGS,device const half*x[[buffer(5)]],device const half*w[[buffer(6)]],device float*y[[buffer(7)]],device const uint*dirty[[buffer(8)]],constant uint&cached[[buffer(9)]],uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 uint row=tile*8,n=jp_count(r,12),base=cached?jp_count(r,1)*n:0;
 if(cached){bool active=false;for(uint j=row;j<min(row+8,n);j++)active|=dirty[j]!=0;if(!active)return;if(lane==0)jp_add(r,102,min(8u,n-row));}
 threadgroup half a[64],b[64];threadgroup float out[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<136;k+=8){for(uint q=lane;q<64;q+=32){uint rr=q/8,cc=q%8;a[q]=row+rr<jp_count(r,12)?x[(base+row+rr)*136+k+cc]:half(0);b[q]=w[(k+rr)*8+cc];}threadgroup_barrier(mem_flags::mem_threadgroup);simdgroup_half8x8 aa,bb;simdgroup_load(aa,a,8);simdgroup_load(bb,b,8);simdgroup_multiply_accumulate(acc,aa,bb,acc);threadgroup_barrier(mem_flags::mem_threadgroup);}
 simdgroup_store(acc,out,8);threadgroup_barrier(mem_flags::mem_threadgroup);for(uint q=lane;q<64;q+=32)if(row+q/8<jp_count(r,12))y[(base+row+q/8)*8+q%8]=out[q];
}
kernel void jp_prediction_finish(JP_ARGS,device float*scores[[buffer(5)]],device const float*scales[[buffer(6)]],device const uint*roots[[buffer(7)]],device const uint*fmap[[buffer(8)]],device uint*priority[[buffer(9)]],constant uint&enabled[[buffer(10)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,12))return;for(uint j=0;j<3;j++){scores[i*8+j]=enabled?scores[i*8+j]*scales[j]+scales[3+j]:1.0f;if(enabled&&scores[i*8+j]>=.5f)jp_add(r,92+j,1);}
 uint f=fmap[roots[i]];if(f&&scores[i*8+2]>=.5f)atomic_store_explicit((device atomic_uint*)(priority+uint(r[(f-1)*STRIDE+5])),1u,memory_order_relaxed);
}
kernel void jp_export_interfaces(JP_ARGS,device const uint*markers[[buffer(5)]],device long*payload[[buffer(6)]],device uint*tables[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,11))return;uint row=markers[i*8],table=uint(r[row*STRIDE]);tables[i]=table;
 for(uint j=0;j<COLS;j++)payload[i*COLS+j]=j<WIDTH[table]?r[row*STRIDE+4+j]:0;
}

// JOINT_REFRESH

// STAGE_KERNELS

// A new ID without a cost witness extends the exact projection by zero rows.
// Cached witness tuples retain Term, i64 cost and result ID together.
kernel void jp_context_extend(JP_ARGS,device uint*counts[[buffer(5)]],device uint*stamps[[buffer(6)]],device uint*ranges[[buffer(7)]],device uint*offsets[[buffer(8)]],constant uint2&cfg[[buffer(9)]],uint i[[thread_position_in_grid]]){
 if(!i){jp_add(r,107,1);jp_add(r,108,cfg.y-cfg.x);}uint at=cfg.x+i;
 if(at<cfg.y){counts[at]=0;stamps[at]=0;ranges[2*at]=0;ranges[2*at+1]=0;}if(at>cfg.x&&at<=cfg.y)offsets[at]=offsets[cfg.x];
}
