// Classify PARTIAL BINDINGS, never the full Cartesian product. Each seed keeps
// its original rule/atom delta ownership. Priorities do not change the snapshot.
// Entry: query-in-packet, event, anchor, unique-prefix-length, interface-id,
//        interface-revision, tier, number-of-covered-atoms.
#define SEED_WORDS (8+P)
bool ready_route(device const uint* code,device uint* rows,device uint* cells,uint route,uint row,thread const uint* v,thread const uint* bound){
 uint cp=code[route],base=(row*NC+cp)*CS;
 if(cells[base]!=2 || cells[base+1]!=R(row,2))return false;
 for(uint j=0;j<code[CELL_META+cp*6+5];j++){uint t=code[route+2+j];if(t!=NO&&bound[t]&&v[t]!=cells[base+CV+j*3])return false;}
 return true;
}
// Same exact ownership checks as cell_segment, without performing or counting
// a query. A certified singleton may delegate the WHOLE seed to its ready port.
uint excluded_route(device const uint* code,device uint* rows,device uint* cells,uint q,uint anchor,uint position,uint route,uint row,uint cutoff,thread uint* v,thread uint* b){
 uint cp=code[route],base=(row*NC+cp)*CS,k=code[route+1];
 for(uint j=0;j<k;j++){
  uint t=code[PLAN+((q*P+anchor)*(P-1)+position+j)*2];
  if(precedes(code,q,t,anchor)&&R(cells[base+CH+j*2],3)>=cutoff)return 1;
 }
 for(uint j=0;j<code[CELL_META+cp*6+5];j++){uint t=code[route+2+j];if(t!=NO){v[t]=cells[base+CV+j*3];b[t]=1;}}
 if(CELL_EVENTS)for(uint j=0;j<k;j++){
  uint t=code[PLAN+((q*P+anchor)*(P-1)+position+j)*2];
  if(port_owns(code,rows,cells,q,anchor,t,cells[base+CH+j*2],v,b))return 2;
 }
 return 0;
}
kernel void family_classify(CTX,uint lane [[thread_position_in_grid]]){
 uint qi=lane/(cfg[8]*P),part=(lane/P)%cfg[8],anchor=lane%P;
 if(qi>=cfg[3])return;uint q=packet[4+qi*4],cutoff=packet[5+qi*4],e=packet[6+qi*4]+part;
 if(e>=cfg[6] || anchor>=code[QMETA+q*3] || !live_event(rows,events,e))return;
 uint row=E(e,0);if(R(row,3)<cutoff)return;
 uint v[S],lo[S],hi[S],b[S];for(uint j=0;j<S;j++){v[j]=lo[j]=hi[j]=b[j]=0;}
 if(!bind_atom(code,rows,q,anchor,row,v,lo,hi,b))return;
 atomic_fetch_add_explicit(routing+24,1,memory_order_relaxed);
 if(port_owns(code,rows,cells,q,anchor,anchor,row,v,b)){atomic_fetch_add_explicit(routing+5,1,memory_order_relaxed);return;}
 uint tier=2,cell=NO,revision=0,covered=0,hintCount=0,hints[P];
 if(cfg[16]>=2 && CELL_CERTIFICATES){
  uint start=CELL_ROUTE+((q*P+anchor)*P)*(2+S),route=start,cp=code[route],candidate=row,position=0;
  // Follow only functionally resolved prefixes. Query length is the bound;
  // ambiguity returns to full search rather than selecting a representative.
  while(cp==NO && position+1<code[QMETA+q*3]){
   bool useful=false;for(uint j=position+1;j<code[QMETA+q*3];j++)if(code[start+j*(2+S)]!=NO){useful=true;break;}
   if(!useful)break;
   uint plan=PLAN+((q*P+anchor)*(P-1)+position)*2,t=code[plan],sp=code[plan+1],at=SPEC+sp*(2+C),atom=ATOM+(q*P+t)*(2+2*C),h=0x31415926u;
   if(code[at+1]==0)break; // No bound coordinate: do not scan a whole relation.
   for(uint j=0;j<code[at+1];j++){uint col=code[at+2+j],kind=code[atom+2+2*col],s=code[atom+3+2*col];h=mixkey(h^(kind?s:v[s]));}
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+26,1,memory_order_relaxed);
   uint found=0,chosen=NO;
   for(uint n=atomic_load_explicit(heads+sp*cfg[4]+(h&(cfg[4]-1)),memory_order_relaxed);n!=NO;n=links[n*NI+sp]){
    if(!live_event(rows,events,n))continue;uint r=E(n,0);if(precedes(code,q,t,anchor)&&R(r,3)>=cutoff)continue;
    uint vv[S],ll[S],hh[S],bb[S];for(uint j=0;j<S;j++){vv[j]=v[j];ll[j]=lo[j];hh[j]=hi[j];bb[j]=b[j];}
    if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+27,1,memory_order_relaxed);
    if(!bind_atom(code,rows,q,t,r,vv,ll,hh,bb))continue;
    found++;chosen=n;if(found==2)break;
   }
   if(found==0){atomic_fetch_add_explicit(routing+4,1,memory_order_relaxed);return;}
   if(found==2){candidate=NO;tier=1;atomic_fetch_add_explicit(routing+6,1,memory_order_relaxed);break;}
   hints[hintCount++]=chosen;candidate=E(chosen,0);bind_atom(code,rows,q,t,candidate,v,lo,hi,b);
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+28,1,memory_order_relaxed);
   position++;route=start+position*(2+S);cp=code[route];
  }
  if(cp!=NO && candidate!=NO){
   cell=candidate*NC+cp;revision=cells[cell*CS+(CELL_EVENTS?2:1)];tier=1;
   if(ready_route(code,rows,cells,route,candidate,v,b)){
    uint excluded=excluded_route(code,rows,cells,q,anchor,position,route,candidate,cutoff,v,b);
    if(excluded){atomic_fetch_add_explicit(routing+(excluded==2?5:27),1,memory_order_relaxed);return;}
    tier=0;covered=code[route+1]+1;
    atomic_fetch_add_explicit(routing+25,covered,memory_order_relaxed);
    atomic_fetch_add_explicit(routing+26,code[QMETA+q*3],memory_order_relaxed);
   }
  }
 }
 uint id=atomic_fetch_add_explicit(routing,1,memory_order_relaxed);
 atomic_fetch_add_explicit(routing+1+tier,1,memory_order_relaxed);
 if(id>=cfg[18]){atomic_store_explicit(routing+7,1,memory_order_relaxed);return;}
 uint at=id*SEED_WORDS;seedQueue[at]=qi;seedQueue[at+1]=e;seedQueue[at+2]=anchor;seedQueue[at+3]=hintCount;
 seedQueue[at+4]=cell;seedQueue[at+5]=revision;seedQueue[at+6]=tier;seedQueue[at+7]=covered;for(uint j=0;j<hintCount;j++)seedQueue[at+8+j]=hints[j];
}
kernel void family_route_dispatch(CTX,uint i [[thread_position_in_grid]]){
 if(i)return;device uint* r=(device uint*)routing;
 for(uint tier=0;tier<3;tier++){uint at=12+tier*4;r[at]=max(1u,(r[1+tier]+127)/128);r[at+1]=1;r[at+2]=1;}
}
kernel void family_route_scatter(CTX,uint i [[thread_position_in_grid]]){
 if(i>=atomic_load_explicit(routing,memory_order_relaxed))return;
 uint tier=seedQueue[i*SEED_WORDS+6],offset=0;for(uint t=0;t<tier;t++)offset+=atomic_load_explicit(routing+1+t,memory_order_relaxed);
 uint at=(offset+atomic_fetch_add_explicit(routing+8+tier,1,memory_order_relaxed))*SEED_WORDS;
 for(uint j=0;j<SEED_WORDS;j++)orderedSeeds[at+j]=seedQueue[i*SEED_WORDS+j];
}
kernel void family_routed_join(CTX,uint lane [[thread_position_in_grid]]){
 uint tier=cfg[17];if(lane>=atomic_load_explicit(routing+1+tier,memory_order_relaxed))return;
 uint offset=0;for(uint t=0;t<tier;t++)offset+=atomic_load_explicit(routing+1+t,memory_order_relaxed);
 uint at=(offset+lane)*SEED_WORDS,cell=orderedSeeds[at+4];
 if(cell!=NO && cells[cell*CS+(CELL_EVENTS?2:1)]!=orderedSeeds[at+5]){atomic_fetch_or_explicit(counters+1,16,memory_order_relaxed);return;}
 uint2 outcome=family_execute(QUERY_PASS,orderedSeeds[at],orderedSeeds[at+1],orderedSeeds[at+2],orderedSeeds+at+8,orderedSeeds[at+3]);
 if(PROFILE_JOIN){
  if(outcome.x)atomic_fetch_add_explicit(counters+29+tier,1,memory_order_relaxed);
  atomic_fetch_add_explicit(counters+32+tier,outcome.x,memory_order_relaxed);
  if(outcome.y)atomic_fetch_add_explicit(counters+35+tier,1,memory_order_relaxed);
  atomic_fetch_add_explicit(counters+38+tier,outcome.y,memory_order_relaxed);
 }
}
