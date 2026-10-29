// Stage-gated private completion. No global table writes until publication.
bool sm_member(uint rule,uint set){for(uint i=SET_START[set];i<SET_START[set+1];i++)if(SET_RULES[i]==rule)return true;return false;}
kernel void sm_count(MS_ARGS,device uint*offset[[buffer(5)]],device uint*blocks[[buffer(6)]],uint lane[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 threadgroup uint sum[256];uint n=lane<cfg[8]?(lane<cfg[1]?ms_visit(cfg[0],lane,cfg[2],rows,offset,0,false):ms_virtual(cfg[0],lane-cfg[1],cfg[2],offset,0,false)):0;
 sum[tid]=n;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sum[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sum[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<cfg[8])offset[lane]=sum[tid]-n;if(tid==255)blocks[group]=sum[tid];
}
kernel void sm_offsets(MS_ARGS,device uint*blocks[[buffer(5)]],uint i[[thread_position_in_grid]]){if(i)return;uint n=0;for(uint g=0;g<(cfg[8]+255)/256;g++){uint v=blocks[g];blocks[g]=n;n+=v;}c[0]=n;}
kernel void sm_pack(MS_ARGS,device const uint*offset[[buffer(5)]],device const uint*blocks[[buffer(6)]],device uint*markers[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i>=cfg[8])return;uint at=offset[i]+blocks[i/256];if(i<cfg[1])ms_visit(cfg[0],i,cfg[2],rows,markers,at,true);else ms_virtual(cfg[0],i-cfg[1],cfg[2],markers,at,true);
}
// Stack frames: kind, limit, body pc, end pc, iteration, current-change, any-change, reserved.
kernel void sm_advance(MS_ARGS,device const uint*code[[buffer(5)]],device uint*stack[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i||c[3])return;
 while(true){uint pc=c[12],op=code[4*pc],arg=code[4*pc+1];
  if(op==0){c[7]=arg;c[12]++;return;}
  if(op==1||op==2){uint end=code[4*pc+2];if(op==1&&!arg){c[12]=end;continue;}
   uint at=8*c[13]++;stack[at]=op;stack[at+1]=arg;stack[at+2]=pc+1;stack[at+3]=end;for(uint k=4;k<8;k++)stack[at+k]=0;c[12]++;continue;}
  if(op==3){if(!c[13]){c[4]=6;c[3]=1;return;}uint at=8*(c[13]-1),changed=stack[at+5];stack[at+4]++;stack[at+6]|=changed;
   bool again=changed&&(stack[at]==2||stack[at+4]<stack[at+1]);
   if(again){stack[at+5]=0;c[12]=stack[at+2];}else{c[12]=stack[at+3];c[13]--;if(c[13])stack[8*(c[13]-1)+5]|=stack[at+6];}continue;}
  if(op==4){c[3]=1;return;}c[4]=6;c[3]=1;return;
 }
}
kernel void sm_apply(MS_ARGS,device const float*state[[buffer(5)]],device float*next[[buffer(6)]],device const uint*markers[[buffer(7)]],device const float*y[[buffer(8)]],device uint*birth[[buffer(9)]],device const uint*hidden[[buffer(10)]],device const uint*stamp[[buffer(11)]],device uint*fired[[buffer(12)]],device const uint*offsets[[buffer(13)]],device const uint*map[[buffer(14)]],device const half*x[[buffer(15)]],uint lane[[thread_position_in_grid]]){
 if(c[3]||lane>=offsets[c[7]+1]-offsets[c[7]])return;uint i=map[offsets[c[7]]+lane],q=i*MS_RECORD,k=markers[q],head=markers[q+1],rule=markers[q+3];if(!sm_member(rule,c[7]))return;
 bool semi=markers[q+4],prediction=y[lane*MS_HEADS+head]>MS_THRESHOLD[head],exact=true;
 if(prediction||cfg[7])for(uint j=0;j<k;j++)exact=exact&&x[lane*MS_WIDTH+1+j]==half(1);
 if((prediction&&!exact)||(cfg[7]&&prediction!=exact)){atomic_store_explicit((device atomic_uint*)(c+4),1u,memory_order_relaxed);return;}
 if(!prediction||(semi&&fired[i]))return;
 uint latest=markers[q+5];for(uint j=0;j<k;j++){uint f=markers[q+6+j];latest=max(latest,birth[f]?c[11]+birth[f]:stamp[f]);}
 if(semi&&latest<=wm[rule])return;fired[i]=1;for(uint j=0;j<k;j++)if(birth[markers[q+6+j]])atomic_fetch_add_explicit((device atomic_uint*)(c+15),1u,memory_order_relaxed);atomic_fetch_add_explicit((device atomic_uint*)(c+6),1u,memory_order_relaxed);
 for(uint j=0;j<markers[q+2];j++){uint f=markers[q+6+MS_K+j];if(hidden[f])continue;
  uint old=atomic_exchange_explicit((device atomic_uint*)(next+f),as_type<uint>(1.0f),memory_order_relaxed);
  if(!old){birth[f]=c[1]+1;atomic_fetch_add_explicit((device atomic_uint*)(c+5),1u,memory_order_relaxed);}}
}
kernel void sm_finish(MS_ARGS,device uint*history[[buffer(5)]],device uint*stack[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i||c[3])return;if(c[1]>=cfg[9]){c[4]=7;c[3]=1;return;}
 uint at=8*c[1],epoch=c[11]+c[1]+1;
 history[at]=c[7];history[at+1]=c[5];history[at+2]=c[6];history[at+3]=epoch;history[at+4]=c[8]+c[2]+c[5];history[at+5]=c[12]-1;history[at+6]=c[13];history[at+7]=c[13]?stack[8*(c[13]-1)+4]:0;
 for(uint j=SET_START[c[7]];j<SET_START[c[7]+1];j++)wm[SET_RULES[j]]=epoch-1;
 c[1]++;c[2]+=c[5];c[14]|=c[5]>0;if(c[13])stack[8*(c[13]-1)+5]|=c[5]>0;if(c[4])c[3]=1;
}
kernel void sm_commit(MS_ARGS,device const uint*history[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(i||c[4])return;m.count=c[8]+c[2];m.next_id=c[9]+c[2];m.next_fact=c[10]+c[2];m.epoch=c[11]+c[1];m.rounds+=c[1];rows[ROW_WORDS+1]=m.epoch;m.changed=c[14];m.matched=c[1]?history[8*(c[1]-1)+2]:0;
}
// One reference per actual marker/set dependency, never rows x all plans.
kernel void sm_stage_count(MS_ARGS,device const uint*markers[[buffer(5)]],device const uint*usedSets[[buffer(6)]],device uint*offsets[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i>=cfg[4])return;uint rule=markers[i*MS_RECORD+3];for(uint k=1;k<=usedSets[0];k++){uint s=usedSets[k];if(sm_member(rule,s))atomic_fetch_add_explicit((device atomic_uint*)(offsets+s),1u,memory_order_relaxed);}
}
kernel void sm_stage_offsets(MS_ARGS,device uint*offsets[[buffer(5)]],device uint*cursor[[buffer(6)]],constant uint&sets[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i)return;uint n=0;for(uint s=0;s<sets;s++){uint v=offsets[s];offsets[s]=n;cursor[s]=n;n+=v;}offsets[sets]=n;c[0]=n;
}
kernel void sm_stage_pack(MS_ARGS,device const uint*markers[[buffer(5)]],device const uint*usedSets[[buffer(6)]],device uint*cursor[[buffer(7)]],device uint*map[[buffer(8)]],uint i[[thread_position_in_grid]]){
 if(i>=cfg[4])return;uint rule=markers[i*MS_RECORD+3];for(uint k=1;k<=usedSets[0];k++){uint s=usedSets[k];if(sm_member(rule,s)){uint at=atomic_fetch_add_explicit((device atomic_uint*)(cursor+s),1u,memory_order_relaxed);map[at]=i;}}
}
kernel void sm_dispatch(MS_ARGS,device const uint*offsets[[buffer(5)]],device uint*dispatch[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i)return;uint n=c[3]?0:offsets[c[7]+1]-offsets[c[7]];
 dispatch[0]=(n+255)/256;dispatch[1]=1;dispatch[2]=1;
 dispatch[3]=((n+7)/8)*(MS_HEADS/8);dispatch[4]=1;dispatch[5]=1;
}
kernel void sm_gather(MS_ARGS,device const float*state[[buffer(5)]],device const uint*markers[[buffer(6)]],device half*x[[buffer(7)]],device const uint*offsets[[buffer(8)]],device const uint*map[[buffer(9)]],uint i[[thread_position_in_grid]]){
 if(c[3]||i>=offsets[c[7]+1]-offsets[c[7]])return;uint q=map[offsets[c[7]]+i]*MS_RECORD;
 for(uint j=0;j<MS_WIDTH;j++)x[i*MS_WIDTH+j]=j==0?half(1):(j<=markers[q]?half(state[markers[q+5+j]]):half(0));
}

kernel void sm_mma(MS_ARGS,device const half*x[[buffer(5)]],device const half*w[[buffer(6)]],device float*y[[buffer(7)]],device const uint*offsets[[buffer(8)]],uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 if(c[3])return;uint cols=MS_HEADS/8,row=(tile/cols)*8,col=(tile%cols)*8;threadgroup half a[64],b[64];threadgroup float out[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<MS_WIDTH;k+=8){for(uint q=lane;q<64;q+=32){uint rr=q/8,cc=q%8;a[q]=row+rr<(offsets[c[7]+1]-offsets[c[7]])?x[(row+rr)*MS_WIDTH+k+cc]:half(0);b[q]=w[(k+rr)*MS_HEADS+col+cc];}
 threadgroup_barrier(mem_flags::mem_threadgroup);simdgroup_half8x8 aa,bb;simdgroup_load(aa,a,8);simdgroup_load(bb,b,8);simdgroup_multiply_accumulate(acc,aa,bb,acc);threadgroup_barrier(mem_flags::mem_threadgroup);}
 simdgroup_store(acc,out,8);threadgroup_barrier(mem_flags::mem_threadgroup);for(uint q=lane;q<64;q+=32){uint rr=row+q/8,cc=col+q%8;if(rr<(offsets[c[7]+1]-offsets[c[7]]))y[rr*MS_HEADS+cc]=out[q];}
}

// Gather completes before apply. Its immutable exact 0/1 columns are the Run
// snapshot, so monotone private outputs need no full-state copy per stage.
kernel void sm_begin(MS_ARGS,uint i[[thread_position_in_grid]]){if(!i&&!c[3]){c[5]=0;c[6]=0;}}
