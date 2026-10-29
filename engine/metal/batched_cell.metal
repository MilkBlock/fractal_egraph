// Three device stages share one resident context/effect matrix. The host queues
// a fixed quantum without reading matches or deciding logical stage transitions.
#define BC_ARGS device long*all_rows[[buffer(0)]],device uint*all_uint[[buffer(1)]],device long*all_long[[buffer(2)]],device Meta*all_meta[[buffer(3)]],device uint*all_state[[buffer(4)]],device long*code[[buffer(5)]],device const uint*tape[[buffer(6)]],device uint*all_history[[buffer(7)]],device long*all_markers[[buffer(8)]],constant uint4&cfg[[buffer(9)]],device long*all_batch[[buffer(10)]],constant uint4&batchCfg[[buffer(11)]],device atomic_uint*dispatch[[buffer(12)]]
kernel void batched_cell_prepare(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device uint*stack=s+8;device long*r=all_rows+instance*GC_ROW_WORDS;device long*old=r;
 // LOCAL_POINTERS
 device uint*p=parent;device float*context_results=(device float*)contexts;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));stats[10]=0;stats[9]=batchCfg.x;
 context_info=(device uint*)stats;
 while(!m.error&&!s[2]){
  uint pc=s[0],op=tape[4*pc],arg=tape[4*pc+1];
  if(op==1||op==2){if(op==1&&!arg){s[0]=tape[pc*4+2];continue;}uint at=8*s[1]++;stack[at]=op;stack[at+1]=arg;stack[at+2]=pc+1;stack[at+3]=tape[pc*4+2];for(uint j=4;j<8;j++)stack[at+j]=0;stack[at+7]=tape[pc*4+3];s[0]++;continue;}
  if(op==3){if(!s[1]){m.error=20;return;}uint at=8*(s[1]-1),changed=stack[at+5];stack[at+4]++;stack[at+6]|=changed;
   if(changed&&(stack[at]==2||stack[at+4]<stack[at+1])){stack[at+5]=0;s[0]=stack[at+2];}else{s[0]=stack[at+3];s[1]--;if(s[1])stack[8*(s[1]-1)+5]|=stack[at+6];}continue;}
  if(op==4){s[2]=1;return;}
  if(op==5){bool found=false;for(uint i=0;i<m.count;i++)found|=r[i*STRIDE]==arg&&r[i*STRIDE+1]==0;if(!found){m.error=8;return;}s[0]++;continue;}
  if(op!=0){m.error=20;return;}
  if(s[3]>=cfg.z){s[4]=1;return;}
  if(!s[7]&&m.epoch==0xffffffff){m.error=6;return;}
  uint set=arg;stats[13]=m.unions;if(!s[7]){m.epoch++;m.rounds++;m.changed=0;}m.matched=0;r[ROW_WORDS+1]=m.epoch;
  bool reset=m.rounds==1||r[QUERY_DIRTY]==1;
  build_query_index(old,reset?0:qheads[2*QBUCKETS],m.count,reset,qheads,qlinks,qprev);
  if(reset)s[5]++;else s[6]++;r[QUERY_LOG]=0;r[QUERY_LOG+1]=m.count;r[QUERY_DIRTY]=0;ensure_row_index(r,p,m);
  if(m.rounds==1)for(uint row=0;row<m.count;row++)r[RC_FACT_ROW+uint(r[row*STRIDE+3])]=row+1;
  rc_compact(contexts,r);bc_reindex_certificates(stats,m.unions,ulong(r[RC_VISIBILITY]));s[7]=1;
  switch(set){// LOCAL_QUERIES
   default:m.error=20;return;
  }
  if(m.error)return;
  if(m.matched>batchCfg.x){m.error=34;return;}
  uint tiles=0,start=0,certificates=uint(stats[15]);device long*meta=stats+BC_STATS;
  while(start<m.matched){uint rule=uint(matches[start*MSTRIDE]),count=0;
   while(start+count<m.matched&&count<BC_TILE&&uint(matches[(start+count)*MSTRIDE])==rule)count++;
   meta[3*tiles]=start;meta[3*tiles+1]=count;meta[3*tiles+2]=bc_select(rule,stack,s[1]);certificates+=count*bc_certificate_bound(int(meta[3*tiles+2]));tiles++;start+=count;
   stats[6]=max(stats[6],long(count));
  }
  if(certificates>batchCfg.x){m.error=34;return;}
  stats[11]=tiles;stats[0]+=tiles;stats[10]=1;s[7]=0;
  atomic_fetch_max_explicit(dispatch,tiles,memory_order_relaxed);return;
 }
}
kernel void batched_cell_evaluate(BC_ARGS,uint2 group[[threadgroup_position_in_grid]],uint tid[[thread_index_in_threadgroup]]){
 uint instance=group.y,tile=group.x;if(instance>=cfg.x)return;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(!stats[10]||tile>=uint(stats[11]))return;
 device long*meta=stats+BC_STATS;uint start=uint(meta[3*tile]),count=uint(meta[3*tile+1]);int plan=int(meta[3*tile+2]);if(plan<0)return;
 device long*data=meta+3*batchCfg.x+start*BC_STRIDE;
 device long*matches=all_long+instance*GC_LONG_WORDS+GC_MATCHES;
 threadgroup float as[256],bs[256],cs[256];
 if(tid<count)bc_prepare(plan,start+tid,matches,data+tid*BC_STRIDE);
 threadgroup_barrier(mem_flags::mem_device|mem_flags::mem_threadgroup);
 bc_matrix(plan,data,count,tid,as,bs,cs);
 threadgroup_barrier(mem_flags::mem_device|mem_flags::mem_threadgroup);
 if(tid<count)bc_evaluate(plan,data+tid*BC_STRIDE);
}
kernel void batched_cell_publish(BC_ARGS,uint instance[[thread_position_in_grid]]){
 if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 device uint*stack=s+8;device long*r=all_rows+instance*GC_ROW_WORDS;device long*old=r;
 // LOCAL_POINTERS
 device uint*p=parent;device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
 if(m.error||!stats[10])return;device long*meta=stats+BC_STATS;device long*data=meta+3*batchCfg.x;
 uint pc=s[0],set=tape[4*pc+1];p[IDCAP]=1;p[IDCAP+1]=0;
 for(uint tile=0;tile<uint(stats[11])&&!m.error;tile++){
  uint start=uint(meta[3*tile]),count=uint(meta[3*tile+1]);int plan=int(meta[3*tile+2]);
  for(uint j=0;j<count&&!m.error;j++){
   uint binding=start+j;device long*o=data+binding*BC_STRIDE;
   if(plan>=0){stats[7]+=o[BC_FAST]!=0;bc_publish(plan,binding,matches,o,r,p,fp,m,stats);}
   else{execute_actions(uint(matches[binding*MSTRIDE]),binding,code,registers,matches,r,p,fp,m);stats[5]++;}
  }
 }
 if(m.error)return;
 p[IDCAP]=0;for(uint j=0;j<p[IDCAP+1];j++)unite_now(p,p[IDCAP+2+2*j],p[IDCAP+3+2*j],m);
 if(m.unions!=uint(stats[13]))rebuild(r,p,fp,m,qheads,qlinks);
 if(m.error)return;
 for(uint j=SET_START[set];j<SET_START[set+1];j++)watermarks[SET_RULES[j]]=m.epoch-1;
 device uint*h=all_history+(instance*cfg.z+s[3])*12;
 h[0]=set;h[1]=m.changed;h[2]=m.matched;h[3]=m.epoch;h[4]=m.count;h[5]=m.unions;h[6]=pc;h[7]=s[1];
 ulong a=0,b=0;
 if(stats[14]){a=1469598103934665603UL;b=0x9e3779b97f4a7c15UL;
 for(uint i=0;i<m.count*STRIDE;i++){ulong x=ulong(r[i]);a=(a^x)*1099511628211UL;b=(b+x)*0xbf58476d1ce4e5b9UL+(b>>31);}
 for(uint i=0;i<m.next_id;i++){a=(a^p[i])*1099511628211UL;b=(b+p[i])*0xbf58476d1ce4e5b9UL;}
 for(uint i=0;i<m.next_fact;i++)a=(a^fp[i])*1099511628211UL;
 for(uint i=0;i<uint(r[ROW_WORDS]);i++)b=(b+ulong(r[ROW_WORDS+i]))*0xbf58476d1ce4e5b9UL;
 }
 h[8]=uint(a);h[9]=uint(a>>32);h[10]=uint(b);h[11]=uint(b>>32);
 s[3]++;s[0]++;stats[10]=0;if(s[1])stack[8*(s[1]-1)+5]|=m.changed;
}
