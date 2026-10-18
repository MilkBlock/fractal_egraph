// Shared native rows, generation-checked tuple indexes, batch-local match output.
#define NO 0xffffffffu
#define RS (4+3*C)
#define OS (1+S)
#define CS (2+2*CK+3*CF)
#define CV (2+2*CK)
#define CTX device const uint* code [[buffer(0)]],device uint* rows [[buffer(1)]],device uint* events [[buffer(2)]],device uint* links [[buffer(3)]],device atomic_uint* heads [[buffer(4)]],device uint* output [[buffer(5)]],device atomic_uint* counters [[buffer(6)]],constant uint* cfg [[buffer(7)]],device const uint* packet [[buffer(8)]],device uint* cells [[buffer(9)]]
#define R(r,c) rows[(r)*RS+(c)]
#define E(e,c) events[(e)*2+(c)]
uint mixkey(uint x){x^=x>>16;x*=0x7feb352du;x^=x>>15;x*=0x846ca68bu;return x^(x>>16);}
kernel void family_patch(CTX,uint i [[thread_position_in_grid]]){
 if(i<cfg[2]){uint at=4+cfg[3]*3+i*(1+RS),slot=packet[at];for(uint j=0;j<RS;j++)R(slot,j)=packet[at+1+j];if(cfg[7]==0){E(cfg[1]+i,0)=slot;E(cfg[1]+i,1)=R(slot,2);}}
}
kernel void family_rebase(CTX,uint i [[thread_position_in_grid]]){if(i<cfg[0]){E(i,0)=i;E(i,1)=R(i,2);}}
kernel void family_clear(CTX,uint i [[thread_position_in_grid]]){if(i<cfg[4]*NI)atomic_store_explicit(heads+i,NO,memory_order_relaxed);}
bool live_event(device uint* rows,device uint* events,uint e){uint r=E(e,0);return R(r,1)!=0 && R(r,2)==E(e,1);}
kernel void family_index(CTX,uint i [[thread_position_in_grid]]){
 uint e=cfg[7]?i:cfg[1]+i;if(e>=cfg[6] || !live_event(rows,events,e))return;
 uint r=E(e,0);
 for(uint sp=0;sp<NI;sp++){
  uint at=SPEC+sp*(2+C);if(code[at]!=R(r,0))continue;uint h=0x31415926u;
  for(uint j=0;j<code[at+1];j++)h=mixkey(h^R(r,4+code[at+2+j]));
  links[e*NI+sp]=atomic_exchange_explicit(heads+sp*cfg[4]+(h&(cfg[4]-1)),e,memory_order_relaxed);
 }
}
bool bind_atom(device const uint* code,device uint* rows,uint q,uint part,uint row,thread uint* v,thread uint* lo,thread uint* hi,thread uint* bound){
 uint at=ATOM+(q*P+part)*(2+2*C);if(R(row,0)!=code[at])return false;
 for(uint col=0;col<code[at+1];col++){
  uint kind=code[at+2+2*col],s=code[at+3+2*col],x=R(row,4+col);
  if(kind==1){if(x!=s)return false;}else{if(bound[s] && v[s]!=x)return false;v[s]=x;lo[s]=R(row,4+C+2*col);hi[s]=R(row,5+C+2*col);bound[s]=1;}
 }return true;
}
void emit_match(device const uint* code,device uint* output,device atomic_uint* counters,constant uint* cfg,uint q,thread const uint* v,thread const uint* lo,thread const uint* hi){
 for(uint j=0;j<code[QMETA+q*3+2];j++){
  uint at=GUARD+(q*G+j)*7,op=code[at];uint2 a=code[at+1]?uint2(code[at+2],code[at+3]):uint2(lo[code[at+2]],hi[code[at+2]]);
  uint2 b=code[at+4]?uint2(code[at+5],code[at+6]):uint2(lo[code[at+5]],hi[code[at+5]]);
  bool lt=nless(a,b),gt=nless(b,a);if(!(op==0?lt:op==1?gt:op==2?!gt:!lt))return;
 }
 uint id=atomic_fetch_add_explicit(counters,1,memory_order_relaxed);
 if(id==NO)atomic_fetch_or_explicit(counters+1,2,memory_order_relaxed);
 if(id>=cfg[5]){atomic_fetch_or_explicit(counters+1,1,memory_order_relaxed);return;}
 output[id*OS]=q;for(uint j=0;j<S;j++)output[id*OS+1+j]=v[j];
}
// Return a certified segment length, 0 for ordinary fallback, NO when
// this branch belongs to an earlier seminaive anchor.
uint cell_segment(device const uint* code,device uint* rows,device uint* cells,device atomic_uint* counters,constant uint* cfg,uint q,uint anchor,uint position,uint r,uint cutoff,thread uint* v,thread uint* lo,thread uint* hi,thread uint* bound){
 if(!CELL_CERTIFICATES)return 0;
 uint route=CELL_ROUTE+((q*P+anchor)*P+position)*(2+S),cp=code[route];if(cp==NO)return 0;
 uint base=(r*NC+cp)*CS,k=code[route+1];
 if(!cells[base] || cells[base+1]!=R(r,2))return 0;
 for(uint j=0;j<k;j++){
  uint t=code[PLAN+((q*P+anchor)*(P-1)+position+j)*2];
  if(t<anchor && R(cells[base+2+j*2],3)>=cutoff)return NO;
 }
 // Outside conditions can further restrict this singleton. On disagreement
 // leave the ordinary query to reject it; never mix two partial bindings.
 for(uint j=0;j<code[CELL_META+cp*6+5];j++){uint t=code[route+2+j];if(t!=NO && bound[t] && v[t]!=cells[base+CV+j*3])return 0;}
 for(uint j=0;j<code[CELL_META+cp*6+5];j++){uint t=code[route+2+j];if(t==NO)continue;v[t]=cells[base+CV+j*3];lo[t]=cells[base+CV+j*3+1];hi[t]=cells[base+CV+j*3+2];bound[t]=1;}
 if(PROFILE_JOIN){atomic_fetch_add_explicit(counters+3,k,memory_order_relaxed);atomic_fetch_add_explicit(counters+4,1,memory_order_relaxed);}
 return k;
}
kernel void family_join(CTX,uint lane [[thread_position_in_grid]]){
 // One launch covers every query of the requested ruleset. Timestamp ownership
 // exactly partitions new matches; no completed-match history is required.
 uint qidx=lane/(cfg[8]*P),part=(lane/P)%cfg[8],anchor=lane%P;
 if(qidx>=cfg[3])return;
 uint q=packet[4+qidx*3],cutoff=packet[5+qidx*3],n=code[QMETA+q*3];
 uint e=packet[6+qidx*3]+part;if(e>=cfg[6])return;if(anchor>=n || !live_event(rows,events,e))return;
 uint r=E(e,0);if(R(r,3)<cutoff)return;
 uint val[P][S],low[P][S],high[P][S],bound[P][S],cursor[P],started[P];int parent[P];
 for(uint s=0;s<S;s++){val[0][s]=0;low[0][s]=0;high[0][s]=0;bound[0][s]=0;}
 if(!bind_atom(code,rows,q,anchor,r,val[0],low[0],high[0],bound[0]))return;
 for(uint d=0;d<P;d++)started[d]=0;
 if(n==1){emit_match(code,output,counters,cfg,q,val[0],low[0],high[0]);return;}
 uint first=cell_segment(code,rows,cells,counters,cfg,q,anchor,0,r,cutoff,val[0],low[0],high[0],bound[0]);
 if(first==NO)return;
 if(first==n-1){emit_match(code,output,counters,cfg,q,val[0],low[0],high[0]);return;}
 if(first)for(uint s=0;s<S;s++){val[first][s]=val[0][s];low[first][s]=low[0][s];high[first][s]=high[0][s];bound[first][s]=bound[0][s];}
 int depth=int(first);parent[depth]=-1;
 while(depth>=0){
  uint step=PLAN+((q*P+anchor)*(P-1)+uint(depth))*2,t=code[step],sp=code[step+1],atom=ATOM+(q*P+t)*(2+2*C);
  if(!started[depth]){
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+2,1,memory_order_relaxed);
   uint at=SPEC+sp*(2+C),h=0x31415926u;
   for(uint j=0;j<code[at+1];j++){uint col=code[at+2+j];uint kind=code[atom+2+2*col],s=code[atom+3+2*col];h=mixkey(h^(kind?s:val[depth][s]));}
   cursor[depth]=atomic_load_explicit(heads+sp*cfg[4]+(h&(cfg[4]-1)),memory_order_relaxed);started[depth]=1;
  }
  uint next=cursor[depth];if(next==NO){started[depth]=0;depth=CELL_CERTIFICATES?parent[depth]:depth-1;continue;}
  cursor[depth]=links[next*NI+sp];if(!live_event(rows,events,next))continue;
  uint row=E(next,0);if(t<anchor && R(row,3)>=cutoff)continue;
  for(uint s=0;s<S;s++){val[depth+1][s]=val[depth][s];low[depth+1][s]=low[depth][s];high[depth+1][s]=high[depth][s];bound[depth+1][s]=bound[depth][s];}
  if(!bind_atom(code,rows,q,t,row,val[depth+1],low[depth+1],high[depth+1],bound[depth+1]))continue;
  uint skip=cell_segment(code,rows,cells,counters,cfg,q,anchor,uint(depth)+1,row,cutoff,val[depth+1],low[depth+1],high[depth+1],bound[depth+1]);
  if(skip==NO)continue;
  uint nextDepth=uint(depth)+1+skip;
  if(nextDepth==n-1)emit_match(code,output,counters,cfg,q,val[depth+1],low[depth+1],high[depth+1]);
  else{
   if(skip)for(uint s=0;s<S;s++){val[nextDepth][s]=val[depth+1][s];low[nextDepth][s]=low[depth+1][s];high[nextDepth][s]=high[depth+1][s];bound[nextDepth][s]=bound[depth+1][s];}
   if(CELL_CERTIFICATES)parent[nextDepth]=depth;depth=int(nextDepth);started[depth]=0;
  }
 }
}
