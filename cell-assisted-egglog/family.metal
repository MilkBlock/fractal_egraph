// Shared native rows, generation-checked tuple indexes, batch-local match output.
#define NO 0xffffffffu
#define RS (4+3*C)
#define OS (1+S)
#define CS (CH+2*CK+3*CF)
#define CV (CH+2*CK)
#define CTX device const uint* code [[buffer(0)]],device uint* rows [[buffer(1)]],device uint* events [[buffer(2)]],device uint* links [[buffer(3)]],device atomic_uint* heads [[buffer(4)]],device uint* output [[buffer(5)]],device atomic_uint* counters [[buffer(6)]],constant uint* cfg [[buffer(7)]],device const uint* packet [[buffer(8)]],device uint* cells [[buffer(9)]],device uint* watches [[buffer(10)]],device atomic_uint* wheads [[buffer(11)]],device uint* dirty [[buffer(12)]],device atomic_uint* state [[buffer(13)]],device uint* ports [[buffer(14)]],device uint* previous [[buffer(15)]],device uint* seedQueue [[buffer(16)]],device uint* orderedSeeds [[buffer(17)]],device atomic_uint* routing [[buffer(18)]],device uint* rowCells [[buffer(19)]],device uint* instanceMeta [[buffer(20)]],device uint* freeCells [[buffer(21)]],device atomic_uint* registry [[buffer(22)]],device atomic_uint* domains [[buffer(23)]]
#define R(r,c) rows[(r)*RS+(c)]
#define E(e,c) events[(e)*3+(c)]
uint index_head(device const uint* packet,constant uint* cfg,uint sp,uint h){uint at=cfg[26]+2*sp;return packet[at]+(h&packet[at+1]);}
uint index_link(device const uint* code,device uint* events,uint e,uint sp){return E(e,2)+code[INDEX_LOCAL+sp];}
uint mixkey(uint x){x^=x>>16;x*=0x7feb352du;x^=x>>15;x*=0x846ca68bu;return x^(x>>16);}
kernel void family_patch(CTX,uint i [[thread_position_in_grid]]){
 if(i<cfg[2]){uint at=4+cfg[3]*4+i*(1+RS),slot=packet[at];for(uint j=0;j<RS;j++)R(slot,j)=packet[at+1+j];if(cfg[7]==0){E(cfg[1]+i,0)=slot;E(cfg[1]+i,1)=R(slot,2);E(cfg[1]+i,2)=packet[cfg[25]+i];}}
}
kernel void family_rebase(CTX,uint i [[thread_position_in_grid]]){if(i<cfg[0]){E(i,0)=i;E(i,1)=R(i,2);E(i,2)=packet[cfg[25]+i];}}
kernel void family_clear(CTX,uint i [[thread_position_in_grid]]){if(i<cfg[4])atomic_store_explicit(heads+i,NO,memory_order_relaxed);}
kernel void family_domain_clear(CTX,uint i [[thread_position_in_grid]]){if(i<cfg[28])atomic_store_explicit(domains+i,0,memory_order_relaxed);}
uint domain_head(device const uint* packet,constant uint* cfg,uint id,uint h){uint at=cfg[27]+2*id;return packet[at]+(h&packet[at+1]);}
bool live_event(device uint* rows,device uint* events,uint e){uint r=E(e,0);return R(r,1)!=0 && R(r,2)==E(e,1);}
// Monotone bounded integer coordinate. The two endpoints mean -/+ infinity,
// so clipping huge i64 values can only weaken refutation, never lose matches.
uint range_coordinate(uint lo,uint hi){
 if((hi==0 && lo<0x80000000u) || (hi==NO && lo>=0x80000000u))return lo^0x80000000u;
 return (hi&0x80000000u)?0u:NO;
}
kernel void family_index(CTX,uint i [[thread_position_in_grid]]){
 uint e=cfg[7]?i:cfg[1]+i;if(e>=cfg[6] || !live_event(rows,events,e))return;
 uint r=E(e,0);
 for(uint si=code[TABLE_SPEC_OFFSETS+R(r,0)];si<code[TABLE_SPEC_OFFSETS+R(r,0)+1];si++){
  uint sp=code[TABLE_SPECS+si];
  uint at=SPEC+sp*(2+C);if(code[at]!=R(r,0))continue;uint h=0x31415926u;
  for(uint j=0;j<code[at+1];j++){uint col=code[at+2+j];
   if(HAS_PRIMITIVES && code[TABLE_I64+R(r,0)*C+col]){h=mixkey(h^R(r,4+C+2*col));h=mixkey(h^R(r,5+C+2*col));}
   else h=mixkey(h^R(r,4+col));
  }
  if(REFUTATION_SUMMARIES)for(uint j=code[DOMAIN_OFFSETS+sp];j<code[DOMAIN_OFFSETS+sp+1];j++){
   uint id=code[DOMAIN_IDS+j],col=code[DOMAIN_META+2*id+1],value;
   if(col!=NO && (col&0xc0000000u)){
    uint c=col&0x3fffffffu,x=range_coordinate(R(r,4+C+2*c),R(r,5+C+2*c));
    // Complemented minimum and maximum both use atomic_max, reset to zero.
    atomic_fetch_max_explicit(domains+domain_head(packet,cfg,id,h),(col&0x40000000u)?~x:x,memory_order_relaxed);continue;
   }
   if(col==NO)value=mixkey(h);
   else if(code[TABLE_I64+R(r,0)*C+col])value=mixkey(mixkey(R(r,4+C+2*col))^R(r,5+C+2*col));
   else value=mixkey(R(r,4+col));
   atomic_fetch_or_explicit(domains+domain_head(packet,cfg,id,h),1u<<(value&31u),memory_order_relaxed);
  }
  links[index_link(code,events,e,sp)]=atomic_exchange_explicit(heads+index_head(packet,cfg,sp,h),e,memory_order_relaxed);
 }
}
bool bind_atom(device const uint* code,device uint* rows,uint q,uint part,uint row,thread uint* v,thread uint* lo,thread uint* hi,thread uint* bound,bool check=true){
 uint at=ATOM+(q*P+part)*(2+2*C);if(R(row,0)!=code[at])return false;
 for(uint col=0;col<code[at+1];col++){
  uint kind=code[at+2+2*col],s=code[at+3+2*col],x=R(row,4+col);
  if(kind==1){if(x!=s)return false;}else{
   if(bound[s]==1 && v[s]!=x)return false;
   if(bound[s]==2 && (lo[s]!=R(row,4+C+2*col)||hi[s]!=R(row,5+C+2*col)))return false;
   v[s]=x;lo[s]=R(row,4+C+2*col);hi[s]=R(row,5+C+2*col);bound[s]=1;}
 }return !check || primitive_constraints(code,q,v,lo,hi,bound,false);
}
// Only observed anchor bindings have a record. No row x plan address space.
uint cell_lookup(device const uint* rowCells,device const uint* instanceMeta,uint row,uint cp){
 for(uint id=rowCells[row];id!=NO;id=instanceMeta[id*3+2])if(instanceMeta[id*3+1]==cp)return id;
 return NO;
}
template<uint QS=S> bool emit_match(device const uint* code,device uint* output,device atomic_uint* counters,constant uint* cfg,uint q,thread const uint* v,thread const uint* lo,thread const uint* hi,bool checked=false){
#if HAS_PRIMITIVES
 if(!checked){
 uint pv[QS],pl[QS],ph[QS],pb[QS];for(uint s=0;s<QS;s++){pv[s]=v[s];pl[s]=lo[s];ph[s]=hi[s];pb[s]=uint(s<code[QMETA+q*3+1]);}
 if(!primitive_constraints(code,q,pv,pl,ph,pb,true))return false;
 }
#endif
 for(uint j=0;j<code[QMETA+q*3+2];j++){
  uint at=GUARD+(q*G+j)*7,op=code[at];uint2 a=code[at+1]?uint2(code[at+2],code[at+3]):uint2(lo[code[at+2]],hi[code[at+2]]);
  uint2 b=code[at+4]?uint2(code[at+5],code[at+6]):uint2(lo[code[at+5]],hi[code[at+5]]);
  bool lt=nless(a,b),gt=nless(b,a);if(!(op==0?lt:op==1?gt:op==2?!gt:!lt))return false;
 }
 uint id=atomic_fetch_add_explicit(counters,1,memory_order_relaxed);
 if(id==NO)atomic_fetch_or_explicit(counters+1,2,memory_order_relaxed);
 uint width=cfg[16]==3?1+code[QMETA+q*3+1]:OS;
 uint at=cfg[16]==3?atomic_fetch_add_explicit(counters+41,width,memory_order_relaxed):id*OS;
 if(at>NO-width){atomic_fetch_or_explicit(counters+1,2,memory_order_relaxed);return true;}
 if(cfg[16]==3?at+width>cfg[5]:id>=cfg[5]){atomic_fetch_or_explicit(counters+1,1,memory_order_relaxed);return true;}
 output[at]=q;for(uint j=0;j<width-1;j++)output[at+1+j]=j<QS?v[j]:0;return true;
}
bool precedes(device const uint* code,uint q,uint a,uint b){return CELL_EVENTS?code[OWNER+q*P+a]<code[OWNER+q*P+b]:a<b;}
// A ready Cell owns this subquery only for internal delta anchors. Outside
// delta anchors have higher priority and keep the ordinary complete query.
bool port_owns(device const uint* code,device uint* rows,device uint* cells,device uint* rowCells,device uint* instanceMeta,uint q,uint anchor,uint part,uint row,thread const uint* v,thread const uint* bound){
 if(!CELL_EVENTS || code[PORT_GROUP+q*P+anchor]==0 || part!=code[PORT_META+q*3+1])return false;
 uint cp=code[PORT_META+q*3];if(cp==NO)return false;uint id=cell_lookup(rowCells,instanceMeta,row,cp);if(id==NO)return false;uint base=id*CS;
 if(cells[base]!=2 || cells[base+1]!=R(row,2))return false;
 for(uint i=0;i<code[CELL_META+cp*6+5];i++){uint t=code[PORT_MAP+q*S+i];if(t!=NO && bound[t] && v[t]!=cells[base+CV+i*3])return false;}
 return true;
}
// Return a certified segment length, 0 for ordinary fallback, NO when
// this branch belongs to an earlier seminaive anchor.
uint cell_segment(device const uint* code,device uint* rows,device uint* cells,device uint* rowCells,device uint* instanceMeta,device atomic_uint* counters,constant uint* cfg,uint q,uint anchor,uint position,uint r,uint cutoff,thread uint* v,thread uint* lo,thread uint* hi,thread uint* bound){
 if(!CELL_CERTIFICATES)return 0;
 uint route=CELL_ROUTE+((q*P+anchor)*P+position)*(2+S),cp=code[route];if(cp==NO)return 0;
 uint id=cell_lookup(rowCells,instanceMeta,r,cp);if(id==NO)return 0;uint base=id*CS,k=code[route+1];
 if(cells[base]!=2 || cells[base+1]!=R(r,2))return 0;
 for(uint j=0;j<k;j++){
  uint t=code[PLAN+((q*P+anchor)*(P-1)+position+j)*2];
  if(precedes(code,q,t,anchor) && R(cells[base+CH+j*2],3)>=cutoff)return NO;
 }
 // Outside conditions can further restrict this singleton. On disagreement
 // leave the ordinary query to reject it; never mix two partial bindings.
 for(uint j=0;j<code[CELL_META+cp*6+5];j++){uint t=code[route+2+j];if(t!=NO && bound[t] && v[t]!=cells[base+CV+j*3])return 0;}
 for(uint j=0;j<code[CELL_META+cp*6+5];j++){uint t=code[route+2+j];if(t==NO)continue;v[t]=cells[base+CV+j*3];lo[t]=cells[base+CV+j*3+1];hi[t]=cells[base+CV+j*3+2];bound[t]=1;}
 // Another cached segment can jump over the port's anchor. It must transfer
 // ownership too, otherwise the direct port lane would emit the same binding.
 if(CELL_EVENTS)for(uint j=0;j<k;j++){
  uint t=code[PLAN+((q*P+anchor)*(P-1)+position+j)*2];
  if(port_owns(code,rows,cells,rowCells,instanceMeta,q,anchor,t,cells[base+CH+j*2],v,bound)){
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+14,1,memory_order_relaxed);return NO;
  }
 }
 if(PROFILE_JOIN){atomic_fetch_add_explicit(counters+3,k,memory_order_relaxed);atomic_fetch_add_explicit(counters+4,1,memory_order_relaxed);}
 return k;
}
#define QUERY_ARGS device const uint* code,device uint* rows,device uint* events,device uint* links,device atomic_uint* heads,device uint* output,device atomic_uint* counters,constant uint* cfg,device const uint* packet,device uint* cells,device uint* rowCells,device uint* instanceMeta,device atomic_uint* domains
#define QUERY_PASS code,rows,events,links,heads,output,counters,cfg,packet,cells,rowCells,instanceMeta,domains
// Return complete bindings emitted and internal atom reads actually reused.
template<uint QP=P,uint QS=S> uint2 family_execute(QUERY_ARGS,uint qidx,uint e,uint anchor,device const uint* hints,uint hintCount){
 uint hits=0,reused=0;if(qidx>=cfg[3])return uint2(0);
 uint q=packet[4+qidx*4],cutoff=packet[5+qidx*4],n=code[QMETA+q*3];
 if(n==0){if(cutoff==0 && anchor==0 && e==0){uint v[QS],l[QS],h[QS];for(uint i=0;i<QS;i++)v[i]=l[i]=h[i]=0;hits+=emit_match<QS>(code,output,counters,cfg,q,v,l,h);}return uint2(hits,0);}
 if(e>=cfg[6] || anchor>=n || !live_event(rows,events,e))return uint2(0);
 uint r=E(e,0);if(R(r,3)<cutoff || R(r,0)!=code[ATOM+(q*P+anchor)*(2+2*C)])return uint2(0);
 uint val[QP][QS],low[QP][QS],high[QP][QS],bound[QP][QS],cursor[QP],started[QP];int parent[QP];
 bool staged=STAGED_CONSTRAINTS && code[PREADY+q]!=0;
 for(uint s=0;s<QS;s++){val[0][s]=0;low[0][s]=0;high[0][s]=0;bound[0][s]=0;}
 if(!bind_atom(code,rows,q,anchor,r,val[0],low[0],high[0],bound[0],!staged) ||
    (staged&&!stage_constraints(code,q,anchor,0,val[0],low[0],high[0],bound[0])))return uint2(hits,reused);
 if(port_owns(code,rows,cells,rowCells,instanceMeta,q,anchor,anchor,r,val[0],bound[0])){if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+14,1,memory_order_relaxed);return uint2(hits,reused);}
 for(uint d=0;d<QP;d++)started[d]=0;
 if(n==1){hits+=emit_match<QS>(code,output,counters,cfg,q,val[0],low[0],high[0],staged);return uint2(hits,reused);}
 uint first=cell_segment(code,rows,cells,rowCells,instanceMeta,counters,cfg,q,anchor,0,r,cutoff,val[0],low[0],high[0],bound[0]);
 if(first==NO)return uint2(hits,reused);
 if(first!=NO)reused+=first;
 if(first==n-1){hits+=emit_match<QS>(code,output,counters,cfg,q,val[0],low[0],high[0]);return uint2(hits,reused);}
 if(first)for(uint s=0;s<QS;s++){val[first][s]=val[0][s];low[first][s]=low[0][s];high[first][s]=high[0][s];bound[first][s]=bound[0][s];}
 int depth=int(first);parent[depth]=-1;
 while(depth>=0){
  uint step=PLAN+((q*P+anchor)*(P-1)+uint(depth))*2,t=code[step],sp=code[step+1],atom=ATOM+(q*P+t)*(2+2*C);
  if(!started[depth]){
   if(PROFILE_JOIN && !(uint(depth)<hintCount))atomic_fetch_add_explicit(counters+2,1,memory_order_relaxed);
   uint at=SPEC+sp*(2+C),h=0x31415926u;
   for(uint j=0;j<code[at+1];j++){uint col=code[at+2+j];uint kind=code[atom+2+2*col],s=code[atom+3+2*col];
    if(HAS_PRIMITIVES && code[TABLE_I64+code[at]*C+col]){
     uint lit=ATOM_LITERAL+(q*P+t)*2*C+2*col;
     h=mixkey(h^(kind?code[lit]:low[depth][s]));h=mixkey(h^(kind?code[lit+1]:high[depth][s]));
    }else h=mixkey(h^(kind?s:val[depth][s]));
   }
   cursor[depth]=(uint(depth)<hintCount)?hints[depth]:atomic_load_explicit(heads+index_head(packet,cfg,sp,h),memory_order_relaxed);started[depth]=1;
  }
  uint next=cursor[depth];if(next==NO){started[depth]=0;depth=CELL_CERTIFICATES?parent[depth]:depth-1;continue;}
  cursor[depth]=(uint(depth)<hintCount)?NO:links[index_link(code,events,next,sp)];if(!live_event(rows,events,next))continue;
  uint row=E(next,0);if(precedes(code,q,t,anchor) && R(row,3)>=cutoff)continue;
  for(uint s=0;s<QS;s++){val[depth+1][s]=val[depth][s];low[depth+1][s]=low[depth][s];high[depth+1][s]=high[depth][s];bound[depth+1][s]=bound[depth][s];}
  if(!bind_atom(code,rows,q,t,row,val[depth+1],low[depth+1],high[depth+1],bound[depth+1],!staged) ||
     (staged&&!stage_constraints(code,q,anchor,uint(depth)+1,val[depth+1],low[depth+1],high[depth+1],bound[depth+1])))continue;
  if(port_owns(code,rows,cells,rowCells,instanceMeta,q,anchor,t,row,val[depth+1],bound[depth+1])){if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+14,1,memory_order_relaxed);continue;}
  uint skip=cell_segment(code,rows,cells,rowCells,instanceMeta,counters,cfg,q,anchor,uint(depth)+1,row,cutoff,val[depth+1],low[depth+1],high[depth+1],bound[depth+1]);
  if(skip==NO)continue;
  reused+=skip;
  uint nextDepth=uint(depth)+1+skip;
  if(nextDepth==n-1)hits+=emit_match<QS>(code,output,counters,cfg,q,val[depth+1],low[depth+1],high[depth+1],staged);
  else{
   if(skip)for(uint s=0;s<QS;s++){val[nextDepth][s]=val[depth+1][s];low[nextDepth][s]=low[depth+1][s];high[nextDepth][s]=high[depth+1][s];bound[nextDepth][s]=bound[depth+1][s];}
   if(CELL_CERTIFICATES)parent[nextDepth]=depth;depth=int(nextDepth);started[depth]=0;
  }
 }
 return uint2(hits,reused);
}
kernel void family_join(CTX,uint lane [[thread_position_in_grid]]){
 uint qidx=lane/(cfg[8]*P),part=(lane/P)%cfg[8],anchor=lane%P;
 if(qidx>=cfg[3])return;uint e=packet[6+qidx*4]+part;
 family_execute(QUERY_PASS,qidx,e,anchor,code,0);
}
