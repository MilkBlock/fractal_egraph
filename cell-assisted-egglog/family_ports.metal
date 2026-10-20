// The old and locality modes use identical ownership and query semantics.
// Every external row is old relative to THIS consumer's native watermark.
#define PORT_ARGS device const uint* code,device uint* rows,device uint* events,device uint* links,device atomic_uint* heads,device uint* output,device atomic_uint* counters,constant uint* cfg,device const uint* packet,device uint* cells,device atomic_uint* state,device uint* ports
#define PORT_PASS code,rows,events,links,heads,output,counters,cfg,packet,cells,state,ports
bool port_accepts(device const uint* code,device const uint* packet,constant uint* cfg,uint qi,uint cp,uint e,uint stamp){
 uint q=packet[4+qi*4],cutoff=packet[5+qi*4];
 return code[PORT_META+q*3]==cp && (cutoff>0 || code[PORT_META+q*3+2]==0) && e>=packet[7+qi*4] && stamp>=cutoff;
}
kernel void family_port_dispatch(CTX,uint i [[thread_position_in_grid]]){
 if(i)return;uint end=atomic_load_explicit(state+2,memory_order_relaxed),span=0;
 for(uint qi=0;qi<cfg[3];qi++){uint q=packet[4+qi*4];if(code[PORT_META+q*3]!=NO && (packet[5+qi*4]>0 || code[PORT_META+q*3+2]==0))span=max(span,end-packet[7+qi*4]);}
 device uint* a=(device uint*)state;a[12]=span;a[13]=end-span;
 a[8]=cfg[15]>0?max(1u,span):max(1u,(span*cfg[3]+127)/128);a[9]=1;a[10]=1;
}
void port_emit(PORT_ARGS,uint qi,uint cp,uint e,uint stamp,thread const uint* v,thread const uint* lo,thread const uint* hi){
 uint q=packet[4+qi*4];
 bool mapped=true;for(uint j=0;j<code[QMETA+q*3+1];j++)if(code[PORT_CANON+q*S+j]==NO)mapped=false;
 if(cfg[15]<2 || !mapped){
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+23,1,memory_order_relaxed);
  bool ok=emit_match(code,output,counters,cfg,q,v,lo,hi);
  if(PROFILE_JOIN&&ok)atomic_fetch_add_explicit(counters+13,1,memory_order_relaxed);return;
 }
 uint groupBase=4+4*cfg[3]+cfg[2]*(1+RS);
 for(uint other=qi;other!=NO;other=packet[groupBase+cfg[3]+other]){
  uint r=packet[4+other*4];
  if(code[PORT_JOIN_GROUP+r]!=code[PORT_JOIN_GROUP+q] || packet[5+other*4]!=packet[5+qi*4] || !port_accepts(code,packet,cfg,other,cp,e,stamp))continue;
  uint vv[S],ll[S],hh[S];for(uint j=0;j<S;j++){vv[j]=ll[j]=hh[j]=0;}
  for(uint j=0;j<code[QMETA+r*3+1];j++){
   uint canonical=code[PORT_CANON+r*S+j],source=canonical==NO?NO:code[PORT_UNCANON+q*S+canonical];
   if(source==NO){atomic_fetch_or_explicit(counters+1,8,memory_order_relaxed);return;}
   vv[j]=v[source];ll[j]=lo[source];hh[j]=hi[source];
  }
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+23,1,memory_order_relaxed);
  bool ok=emit_match(code,output,counters,cfg,r,vv,ll,hh);
  if(PROFILE_JOIN&&ok)atomic_fetch_add_explicit(counters+13,1,memory_order_relaxed);
 }
}
void port_query(PORT_ARGS,uint qi,uint cp,uint e,uint stamp,thread const uint* context){
 uint q=packet[4+qi*4],cutoff=packet[5+qi*4],n=code[PORT_META+q*3+2];
 if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+22,1,memory_order_relaxed);
 uint val[P+1][S],low[P+1][S],high[P+1][S],bound[P+1][S],cursor[P],started[P];
 for(uint s=0;s<S;s++){val[0][s]=low[0][s]=high[0][s]=bound[0][s]=0;}
 for(uint i=0;i<code[CELL_META+cp*6+5];i++){uint t=code[PORT_MAP+q*S+i];if(t==NO)continue;val[0][t]=context[i*3];low[0][t]=context[i*3+1];high[0][t]=context[i*3+2];bound[0][t]=1;}
 if(n==0){port_emit(PORT_PASS,qi,cp,e,stamp,val[0],low[0],high[0]);return;}
 for(uint j=0;j<P;j++)started[j]=0;int depth=0;
 while(depth>=0){
  uint plan=PORT_PLAN+(q*P+uint(depth))*2,t=code[plan],sp=code[plan+1],atom=ATOM+(q*P+t)*(2+2*C);
  if(!started[depth]){
   uint at=SPEC+sp*(2+C),h=0x31415926u;for(uint j=0;j<code[at+1];j++){uint col=code[at+2+j],kind=code[atom+2+2*col],s=code[atom+3+2*col];h=mixkey(h^(kind?s:val[depth][s]));}
   cursor[depth]=atomic_load_explicit(heads+index_head(packet,cfg,sp,h),memory_order_relaxed);started[depth]=1;
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+15,1,memory_order_relaxed);
  }
  uint candidate=cursor[depth];if(candidate==NO){started[depth]=0;depth--;continue;}cursor[depth]=links[index_link(code,events,candidate,sp)];if(!live_event(rows,events,candidate))continue;
  uint row=E(candidate,0);if(R(row,3)>=cutoff)continue;
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+25,1,memory_order_relaxed);
  for(uint s=0;s<S;s++){val[depth+1][s]=val[depth][s];low[depth+1][s]=low[depth][s];high[depth+1][s]=high[depth][s];bound[depth+1][s]=bound[depth][s];}
  if(!bind_atom(code,rows,q,t,row,val[depth+1],low[depth+1],high[depth+1],bound[depth+1]))continue;
  if(depth==int(n)-1)port_emit(PORT_PASS,qi,cp,e,stamp,val[depth+1],low[depth+1],high[depth+1]);
  else{depth++;started[depth]=0;}
 }
}
kernel void family_port_join(CTX,uint lane [[thread_position_in_grid]]){
 uint span=((device uint*)state)[12];if(!span)return;uint qi=lane/span,e=lane%span;if(qi>=cfg[3])return;
 e+=packet[7+qi*4];if(e>=atomic_load_explicit(state+2,memory_order_relaxed))return;
 uint id=ports[e*2],cp=instanceMeta[id*3+1],base=id*CS;
 if(cells[base]!=2 || cells[base+6]!=ports[e*2+1] || !port_accepts(code,packet,cfg,qi,cp,e,cells[base+7]))return;
 uint context[3*S],fields=code[CELL_META+cp*6+5];
 for(uint j=0;j<fields*3;j++)context[j]=cells[base+CV+j];
 if(PROFILE_JOIN){atomic_fetch_add_explicit(counters+20,1,memory_order_relaxed);atomic_fetch_add_explicit(counters+21,fields*3,memory_order_relaxed);}
 port_query(PORT_PASS,qi,cp,e,cells[base+7],context);
}
// One workgroup owns one ready Cell port. Its complete interface frame is read
// once into shared memory. Independent query groups execute in parallel lanes.
kernel void family_port_locality(CTX,uint group [[threadgroup_position_in_grid]],uint tid [[thread_index_in_threadgroup]],uint width [[threads_per_threadgroup]]){
 uint span=((device uint*)state)[12];if(group>=span)return;
 uint e=((device uint*)state)[13]+group,id=ports[e*2],cp=instanceMeta[id*3+1],base=id*CS;
 if(cells[base]!=2 || cells[base+6]!=ports[e*2+1])return;
 uint stamp=cells[base+7],fields=code[CELL_META+cp*6+5];
 threadgroup uint common[3*S];threadgroup uint needed;
 if(tid==0){needed=0;for(uint qi=0;qi<cfg[3];qi++)if(port_accepts(code,packet,cfg,qi,cp,e,stamp)){needed=1;break;}}
 threadgroup_barrier(mem_flags::mem_threadgroup);if(!needed)return;
 for(uint j=tid;j<fields*3;j+=width)common[j]=cells[base+CV+j];
 if(tid==0&&PROFILE_JOIN){atomic_fetch_add_explicit(counters+20,1,memory_order_relaxed);atomic_fetch_add_explicit(counters+21,fields*3,memory_order_relaxed);}
 threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint qi=tid;qi<cfg[3];qi+=width){
  if(!port_accepts(code,packet,cfg,qi,cp,e,stamp))continue;
  uint groupBase=4+4*cfg[3]+cfg[2]*(1+RS);
  if(cfg[15]>=2 && packet[groupBase+qi]!=qi){if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+24,1,memory_order_relaxed);continue;}
  uint context[3*S];for(uint j=0;j<fields*3;j++)context[j]=common[j];
  port_query(PORT_PASS,qi,cp,e,stamp,context);
 }
}
