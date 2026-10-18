// Commit-driven interface states. Headers: state, anchor generation, watch
// revision, queued epoch, dirty step, completed steps, port revision, timestamp.
// Watches include the first MISSING key, so a late fact resumes the same state.
#define WS (6+C)
#define W(i,j) watches[(i)*WS+(j)]
uint watch_hash(uint table,uint width,thread const uint* key){uint h=mixkey(0x31415926u^table);for(uint j=0;j<width;j++)h=mixkey(h^key[j]);return h;}
void dirty_cell(device uint* cells,device uint* dirty,device atomic_uint* state,constant uint* cfg,uint id,uint step){
 uint base=id*CS;atomic_fetch_min_explicit((device atomic_uint*)(cells+base+4),step,memory_order_relaxed);
 if(atomic_exchange_explicit((device atomic_uint*)(cells+base+3),cfg[9],memory_order_relaxed)!=cfg[9]){
  uint at=atomic_fetch_add_explicit(state+1,1,memory_order_relaxed);dirty[at]=id;
 }
}
void wake_key(device uint* cells,device uint* watches,device atomic_uint* wheads,device uint* dirty,device atomic_uint* state,device atomic_uint* counters,constant uint* cfg,uint table,uint width,thread const uint* key,uint hint,uint generation){
 uint bucket=watch_hash(table,width,key)&(cfg[10]-1);
 for(uint w=atomic_load_explicit(wheads+bucket,memory_order_relaxed);w!=NO;w=W(w,5)){
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+18,1,memory_order_relaxed);
  uint id=W(w,0);if(cells[id*CS+2]!=W(w,1) || W(w,3)!=table || W(w,4)!=width)continue;
  bool equal=true;for(uint j=0;j<width;j++)if(W(w,6+j)!=key[j]){equal=false;break;}
  if(equal){if(hint!=NO){uint at=id*CS+8+W(w,2)*2;cells[at]=hint;cells[at+1]=generation;}dirty_cell(cells,dirty,state,cfg,id,W(w,2));if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+10,1,memory_order_relaxed);}
 }
}
kernel void family_wake(CTX,uint i [[thread_position_in_grid]]){
 if(i>=cfg[2])return;uint at=4+cfg[3]*4+i*(1+RS),r=packet[at];
 uint key[C];
 if(r<cfg[11] && R(r,1)){
  uint table=R(r,0),width=code[TABLE_KEYS+table];for(uint j=0;j<width;j++)key[j]=R(r,4+j);
  wake_key(cells,watches,wheads,dirty,state,counters,cfg,table,width,key,NO,0);
 }
 if(packet[at+2]){
  uint table=packet[at+1],width=code[TABLE_KEYS+table];for(uint j=0;j<width;j++)key[j]=packet[at+5+j];
  wake_key(cells,watches,wheads,dirty,state,counters,cfg,table,width,key,r,packet[at+3]);
 }
 for(uint cp=0;cp<NC;cp++){
  uint m=CELL_META+cp*6,q=code[m],a=code[m+1],table=code[ATOM+(q*P+a)*(2+2*C)];
  if((r<cfg[11] && R(r,0)==table) || packet[at+1]==table)dirty_cell(cells,dirty,state,cfg,r*NC+cp,0);
 }
}
kernel void family_watch_clear(CTX,uint i [[thread_position_in_grid]]){if(i<cfg[10])atomic_store_explicit(wheads+i,NO,memory_order_relaxed);}
kernel void family_watch_compact(CTX,uint i [[thread_position_in_grid]]){
 if(i>=cfg[12])return;uint at=i*WS,id=previous[at];if(cells[id*CS+2]!=previous[at+1])return;
 uint w=atomic_fetch_add_explicit(state,1,memory_order_relaxed);for(uint j=0;j<WS;j++)W(w,j)=previous[at+j];
 uint key[C];for(uint j=0;j<W(w,4);j++)key[j]=W(w,6+j);
 uint bucket=watch_hash(W(w,3),W(w,4),key)&(cfg[10]-1);W(w,5)=atomic_exchange_explicit(wheads+bucket,w,memory_order_relaxed);
}
void register_key(device uint* watches,device atomic_uint* wheads,device atomic_uint* state,constant uint* cfg,uint id,uint revision,uint step,uint table,uint width,thread const uint* key){
 uint w=atomic_fetch_add_explicit(state,1,memory_order_relaxed);W(w,0)=id;W(w,1)=revision;W(w,2)=step;W(w,3)=table;W(w,4)=width;
 for(uint j=0;j<width;j++)W(w,6+j)=key[j];
 uint bucket=watch_hash(table,width,key)&(cfg[10]-1);W(w,5)=atomic_exchange_explicit(wheads+bucket,w,memory_order_relaxed);
}
kernel void family_cell_dispatch(CTX,uint i [[thread_position_in_grid]]){
 if(i)return;uint n=atomic_load_explicit(state+1,memory_order_relaxed);device uint* a=(device uint*)state;a[4]=max(1u,(n+127)/128);a[5]=1;a[6]=1;
}
kernel void family_cell_events(CTX,uint lane [[thread_position_in_grid]]){
 if(lane>=atomic_load_explicit(state+1,memory_order_relaxed))return;
 uint id=dirty[lane],r=id/NC,cp=id%NC,base=id*CS,m=CELL_META+cp*6,q=code[m],anchor=code[m+1],k=code[m+2];
 if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+11,1,memory_order_relaxed);
 uint resume=min(cells[base+4],cells[base+5]);cells[base+4]=NO;
 if(cells[base+1]!=R(r,2))resume=0;
 if(cells[base]==2 && PROFILE_JOIN)atomic_fetch_add_explicit(counters+8,1,memory_order_relaxed);
 if(cells[base+2]==NO || cells[base+6]==NO){atomic_fetch_or_explicit(counters+1,4,memory_order_relaxed);return;}
 cells[base+2]++;cells[base+6]++;cells[base]=0;cells[base+5]=0;cells[base+1]=R(r,2);
 if(!R(r,1))return;
 uint v[S],lo[S],hi[S],b[S];for(uint j=0;j<S;j++){v[j]=lo[j]=hi[j]=b[j]=0;}
 if(!bind_atom(code,rows,q,anchor,r,v,lo,hi,b))return;
 if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+5,1,memory_order_relaxed);
 uint stamp=R(r,3);
 for(uint j=0;j<k;j++){
  uint step=CELL_STEP+(cp*(P-1)+j)*2,t=code[step],sp=code[step+1],atom=ATOM+(q*P+t)*(2+2*C),table=code[atom],width=code[TABLE_KEYS+table],key[C];
  for(uint c=0;c<width;c++){uint kind=code[atom+2+2*c],s=code[atom+3+2*c];key[c]=kind?s:v[s];}
  register_key(watches,wheads,state,cfg,id,cells[base+2],j,table,width,key);
  uint save=base+8+j*2,chosen=NO;
  if(cells[save+1]!=0 && R(cells[save],1) && R(cells[save],2)==cells[save+1]){
   uint vv[S],ll[S],hh[S],bb[S];for(uint s=0;s<S;s++){vv[s]=v[s];ll[s]=lo[s];hh[s]=hi[s];bb[s]=b[s];}
   if(bind_atom(code,rows,q,t,cells[save],vv,ll,hh,bb)){
    chosen=cells[save];if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+(j<resume?12:19),1,memory_order_relaxed);
   }
  }
  if(chosen==NO){
   uint at=SPEC+sp*(2+C),h=0x31415926u;
   for(uint c=0;c<code[at+1];c++){uint col=code[at+2+c],kind=code[atom+2+2*col],s=code[atom+3+2*col];h=mixkey(h^(kind?s:v[s]));}
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+9,1,memory_order_relaxed);
   for(uint e=atomic_load_explicit(heads+sp*cfg[4]+(h&(cfg[4]-1)),memory_order_relaxed);e!=NO;e=links[e*NI+sp]){
    if(!live_event(rows,events,e))continue;
    uint vv[S],ll[S],hh[S],bb[S];for(uint s=0;s<S;s++){vv[s]=v[s];ll[s]=lo[s];hh[s]=hi[s];bb[s]=b[s];}
    if(bind_atom(code,rows,q,t,E(e,0),vv,ll,hh,bb)){chosen=E(e,0);break;}
   }
  }
  if(chosen==NO){cells[base]=1;if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+17,1,memory_order_relaxed);return;}
  if(!bind_atom(code,rows,q,t,chosen,v,lo,hi,b)){cells[base]=1;return;}
  cells[save]=chosen;cells[save+1]=R(chosen,2);cells[base+5]=j+1;stamp=max(stamp,R(chosen,3));
 }
 bool shape=false;for(uint a=0;a<code[m+4];a++){
  uint at=CELL_VARIANT+code[m+3]+a*2*S;bool good=true;
  for(uint i=0;i<S;i++){uint kind=code[at+2*i],x=code[at+2*i+1];if((kind==1 && v[i]!=v[x]) || (kind==2 && v[i]!=x)){good=false;break;}}
  if(good){shape=true;break;}
 }
 if(!shape){cells[base]=3;if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+7,1,memory_order_relaxed);return;}
 for(uint i=0;i<code[m+5];i++){uint f=code[CELL_FIELDS+cp*S+i];cells[base+CV+i*3]=v[f];cells[base+CV+i*3+1]=lo[f];cells[base+CV+i*3+2]=hi[f];}
 cells[base]=2;cells[base+7]=stamp;
 uint at=atomic_fetch_add_explicit(state+2,1,memory_order_relaxed);ports[at*2]=id;ports[at*2+1]=cells[base+6];
 if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+16,1,memory_order_relaxed);
}
kernel void family_port_compact(CTX,uint i [[thread_position_in_grid]]){
 if(i>=cfg[14])return;uint id=previous[i*2],version=previous[i*2+1];if(cells[id*CS]!=2 || cells[id*CS+6]!=version)return;
 uint at=atomic_fetch_add_explicit(state+2,1,memory_order_relaxed);ports[at*2]=id;ports[at*2+1]=version;
}
