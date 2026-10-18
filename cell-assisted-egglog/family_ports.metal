// Ready-port deltas drive a residual query. All residual rows must be old;
// matches with any new external atom belong to the ordinary boundary-delta lane.
kernel void family_port_dispatch(CTX,uint i [[thread_position_in_grid]]){
 if(i)return;uint end=atomic_load_explicit(state+2,memory_order_relaxed),span=0;
 for(uint i=0;i<cfg[3];i++){uint q=packet[4+i*4];if(code[PORT_META+q*3]!=NO && (packet[5+i*4]>0 || code[PORT_META+q*3+2]==0))span=max(span,end-packet[7+i*4]);}
 device uint* a=(device uint*)state;a[12]=span;a[8]=max(1u,(span*cfg[3]+127)/128);a[9]=1;a[10]=1;
}
kernel void family_port_join(CTX,uint lane [[thread_position_in_grid]]){
 uint span=((device uint*)state)[12];if(!span)return;uint qi=lane/span,e=lane%span;if(qi>=cfg[3])return;
 uint q=packet[4+qi*4],cutoff=packet[5+qi*4];if(code[PORT_META+q*3]==NO || (cutoff==0 && code[PORT_META+q*3+2]>0))return;e+=packet[7+qi*4];if(e>=atomic_load_explicit(state+2,memory_order_relaxed))return;
 uint id=ports[e*2],cp=id%NC,base=id*CS;
 if(cp!=code[PORT_META+q*3] || cells[base]!=2 || cells[base+6]!=ports[e*2+1] || cells[base+7]<cutoff)return;
 uint n=code[PORT_META+q*3+2],val[P+1][S],low[P+1][S],high[P+1][S],bound[P+1][S],cursor[P],started[P];
 for(uint s=0;s<S;s++){val[0][s]=low[0][s]=high[0][s]=bound[0][s]=0;}
 for(uint i=0;i<code[CELL_META+cp*6+5];i++){uint t=code[PORT_MAP+q*S+i];if(t==NO)continue;val[0][t]=cells[base+CV+i*3];low[0][t]=cells[base+CV+i*3+1];high[0][t]=cells[base+CV+i*3+2];bound[0][t]=1;}
 if(n==0){bool accepted=emit_match(code,output,counters,cfg,q,val[0],low[0],high[0]);if(PROFILE_JOIN && accepted)atomic_fetch_add_explicit(counters+13,1,memory_order_relaxed);return;}
 for(uint j=0;j<P;j++)started[j]=0;int depth=0;
 while(depth>=0){
  uint plan=PORT_PLAN+(q*P+uint(depth))*2,t=code[plan],sp=code[plan+1],atom=ATOM+(q*P+t)*(2+2*C);
  if(!started[depth]){
   uint at=SPEC+sp*(2+C),h=0x31415926u;for(uint j=0;j<code[at+1];j++){uint col=code[at+2+j],kind=code[atom+2+2*col],s=code[atom+3+2*col];h=mixkey(h^(kind?s:val[depth][s]));}
   cursor[depth]=atomic_load_explicit(heads+sp*cfg[4]+(h&(cfg[4]-1)),memory_order_relaxed);started[depth]=1;
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+15,1,memory_order_relaxed);
  }
  uint e=cursor[depth];if(e==NO){started[depth]=0;depth--;continue;}cursor[depth]=links[e*NI+sp];if(!live_event(rows,events,e))continue;
  uint row=E(e,0);if(R(row,3)>=cutoff)continue;
  for(uint s=0;s<S;s++){val[depth+1][s]=val[depth][s];low[depth+1][s]=low[depth][s];high[depth+1][s]=high[depth][s];bound[depth+1][s]=bound[depth][s];}
  if(!bind_atom(code,rows,q,t,row,val[depth+1],low[depth+1],high[depth+1],bound[depth+1]))continue;
  if(depth==int(n)-1){bool accepted=emit_match(code,output,counters,cfg,q,val[depth+1],low[depth+1],high[depth+1]);if(PROFILE_JOIN && accepted)atomic_fetch_add_explicit(counters+13,1,memory_order_relaxed);}
  else{depth++;started[depth]=0;}
 }
}
