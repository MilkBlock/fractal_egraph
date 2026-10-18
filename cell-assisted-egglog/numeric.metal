// Typed i64 matrix operators. No i64 -> float or intern-ID -> integer conversion.
#define NL(c,f) U(NUMERIC_LANES,c,NUMERIC_LANES_##f)
#define NE(n,f) U(NUMERIC_OPS,n,NUMERIC_OPS_##f)
#define NG(g,f) U(NUMERIC_GUARDS,g,NUMERIC_GUARDS_##f)
#define NW(w,f) U(NUMERIC_WRITES,w,NUMERIC_WRITES_##f)
#define NP(w,f) U(NUMERIC_PROPOSALS,w,NUMERIC_PROPOSALS_##f)
#define NS(s,f) U(NUMERIC_STATE,s,NUMERIC_STATE_##f)
void ninc(device uint *h,uint field,uint count=1) {
  atomic_fetch_add_explicit((device atomic_uint*)(h+NUMERIC_COUNTERS_OFF+field),count,memory_order_relaxed);
}
bool nless(uint2 a,uint2 b) {int x=as_type<int>(a.y),y=as_type<int>(b.y);return x<y || (x==y && a.x<b.x);}
uint2 nneg(uint2 a) {uint lo=0u-a.x;return uint2(lo,~a.y+uint(a.x==0));}
uint3 nadd(uint2 a,uint2 b) {
  uint lo=a.x+b.x;uint hi=a.y+b.y+uint(lo<a.x);
  bool ok=((a.y^hi)&(b.y^hi)&0x80000000u)==0;return uint3(lo,hi,uint(ok));
}
uint3 nsub(uint2 a,uint2 b) {
  uint lo=a.x-b.x,hi=a.y-b.y-uint(a.x<b.x);
  bool ok=((a.y^b.y)&(a.y^hi)&0x80000000u)==0;return uint3(lo,hi,uint(ok));
}
// Explicit 16-bit partial products avoid an AGX compiler failure on mulhi in
// this dynamic operator graph. All intermediates here fit unsigned 32 bits.
uint nhi(uint a,uint b) {
  uint a0=a&65535u,a1=a>>16,b0=b&65535u,b1=b>>16;
  uint low=a0*b0,t=a1*b0+(low>>16),middle=(t&65535u)+a0*b1;
  return a1*b1+(t>>16)+(middle>>16);
}
uint3 nmul(uint2 a,uint2 b) {
  bool neg=((a.y^b.y)&0x80000000u)!=0;
  if(a.y&0x80000000u)a=nneg(a);if(b.y&0x80000000u)b=nneg(b);
  uint lo=a.x*b.x,hi=nhi(a.x,b.x),p=a.x*b.y,q=a.y*b.x;
  bool ok=!(a.y && b.y) && nhi(a.x,b.y)==0 && nhi(a.y,b.x)==0;
  uint next=hi+p;ok&=next>=hi;hi=next;next=hi+q;ok&=next>=hi;hi=next;
  ok&=hi<0x80000000u || (neg && hi==0x80000000u && lo==0);
  uint2 value=uint2(lo,hi);return uint3(neg?nneg(value):value,uint(ok));
}
uint2 nvalue(device uint*h,uint lane,uint node){return uint2(U(NUMERIC_VALUES,lane,node*2),U(NUMERIC_VALUES,lane,node*2+1));}
void neval(device uint*h,uint lane) {
  if(U(NUMERIC_VALID,lane,0))return;
  for(uint n=0;n<NUMERIC_OPS_ROWS;n++) {
    uint op=NE(n,OP),slot=NE(n,SLOT),a=NE(n,LEFT),b=NE(n,RIGHT);bool ok=true;uint2 v;
    if(op==0) v=uint2(U(NUMERIC_INPUTS,lane,slot*2),U(NUMERIC_INPUTS,lane,slot*2+1));
    else if(op==1) {uint s=U(NUMERIC_STATE_BINDINGS,lane,slot);v=uint2(NS(s,LO),NS(s,HI));ok=NS(s,PRESENT)!=0;}
    else if(op==2)v=uint2(NE(n,LO),NE(n,HI));
    else {
      uint2 x=nvalue(h,lane,a),y=nvalue(h,lane,b);bool arithmetic=true;
      if(op>=3 && op<=5){uint3 checked=op==3?nadd(x,y):op==4?nsub(x,y):nmul(x,y);v=checked.xy;arithmetic=checked.z!=0;}
      else if(op==6)v=nless(x,y)?x:y;else v=nless(x,y)?y:x;
      ok=arithmetic && U(NUMERIC_DEFINED,lane,a) && U(NUMERIC_DEFINED,lane,b);
    }
    U(NUMERIC_VALUES,lane,n*2)=v.x;U(NUMERIC_VALUES,lane,n*2+1)=v.y;U(NUMERIC_DEFINED,lane,n)=ok;
  }
  U(NUMERIC_VALID,lane,0)=1;ninc(h,NUMERIC_COUNTERS_EXPRESSIONS,NUMERIC_OPS_ROWS);
}
uint numeric_guard_state(device uint*h,uint ci) {
  bool pending=false;
  for(uint j=0;j<NL(ci,GUARD_COUNT);j++){
    uint g=U(NUMERIC_GUARD_INDEX,NL(ci,GUARD_START)+j,0),v=U(NUMERIC_EVIDENCE,g,0);
    if(v==1)return 1;pending|=v==0;
  }return pending?0:2;
}
kernel void numeric_begin(CONTEXT,uint i [[thread_position_in_grid]]) {
  if(!NUMERIC_ENABLED || i>=cfg.y)return;
  uint c=U(STAGE_CHANNELS,cfg.x+i,0);U(NUMERIC_VALID,c,0)=0;
  for(uint j=0;j<NL(c,GUARD_COUNT);j++)U(NUMERIC_EVIDENCE,U(NUMERIC_GUARD_INDEX,NL(c,GUARD_START)+j,0),0)=0;
  for(uint j=0;j<NL(c,WRITE_COUNT);j++){uint w=U(NUMERIC_WRITE_INDEX,NL(c,WRITE_START)+j,0);NP(w,STATUS)=0;NP(w,COMMITTED)=0;NP(w,LO)=0;NP(w,HI)=0;}
}
kernel void numeric_guard(CONTEXT,uint i [[thread_position_in_grid]]) {
  if(!NUMERIC_ENABLED || i>=cfg.y)return;
  uint c=U(STAGE_CHANNELS,cfg.x+i,0);if(U(DECISIONS,c,0)!=0)return;
  for(uint j=0;j<CH(c,SOURCE_COUNT);j++)if(U(LIVE,U(SOURCES,CH(c,SOURCE_START)+j,0),0)!=1)return;
  for(uint j=0;j<NL(c,GUARD_COUNT);j++){
    uint g=U(NUMERIC_GUARD_INDEX,NL(c,GUARD_START)+j,0);
    if(U(NUMERIC_EVIDENCE,g,0)!=0 || NG(g,DEPTH)>cfg.z)continue;
    neval(h,c);uint a=NG(g,LEFT),b=NG(g,RIGHT),op=NG(g,COMPARE);uint2 x=nvalue(h,c,a),y=nvalue(h,c,b);
    bool lt=nless(x,y),gt=nless(y,x),eq=all(x==y);
    bool pass=(op==0?lt:op==1?!gt:op==2?gt:op==3?!lt:op==4?eq:!eq) && U(NUMERIC_DEFINED,c,a) && U(NUMERIC_DEFINED,c,b);
    U(NUMERIC_EVIDENCE,g,0)=pass?2:1;ninc(h,NUMERIC_COUNTERS_GUARDS);
  }
}
kernel void numeric_clear_selected(CONTEXT,uint i [[thread_position_in_grid]]) {
  if(i<NUMERIC_SELECTED_ROWS)U(NUMERIC_SELECTED,i,0)=0;
}
kernel void numeric_propose(CONTEXT,uint i [[thread_position_in_grid]]) {
  if(!NUMERIC_ENABLED || i>=cfg.y)return;
  uint c=U(STAGE_CHANNELS,cfg.x+i,0);U(NUMERIC_SELECTED,c,0)=1;
  if(U(DECISIONS,c,0)!=1)return;
  for(uint j=0;j<NL(c,WRITE_COUNT);j++) {
    uint w=U(NUMERIC_WRITE_INDEX,NL(c,WRITE_START)+j,0);if(NP(w,COMMITTED))continue;
    neval(h,c);uint node=NW(w,VALUE);bool valid=U(NUMERIC_DEFINED,c,node)!=0;uint2 v=nvalue(h,c,node);
    NP(w,LO)=v.x;NP(w,HI)=v.y;NP(w,STATUS)=valid?1:2;ninc(h,NUMERIC_COUNTERS_PROPOSALS);
    if(!valid)atomic_store_explicit((device atomic_uint*)(h+NUMERIC_ERROR_OFF),1,memory_order_relaxed);
  }
}
kernel void numeric_reduce(CONTEXT,uint g [[thread_position_in_grid]]) {
  if(g>=NUMERIC_GROUPS_ROWS || U(NUMERIC_ERROR,0,0))return;
  uint2 old=uint2(NS(g,LO),NS(g,HI)),value=old;bool present=NS(g,PRESENT)!=0,used=false;
  uint begin=U(NUMERIC_GROUPS,g,NUMERIC_GROUPS_START),count=U(NUMERIC_GROUPS,g,NUMERIC_GROUPS_COUNT),op=U(NUMERIC_GROUPS,g,NUMERIC_GROUPS_MERGE);
  for(uint j=0;j<count;j++) {
    uint w=U(NUMERIC_GROUP_INDEX,begin+j,0);if(NP(w,COMMITTED) || NP(w,STATUS)!=1 || U(DECISIONS,NW(w,CHANNEL),0)!=1)continue;
    // Only this stage's snapshot proposals are eligible.
    if(!U(NUMERIC_SELECTED,NW(w,CHANNEL),0))continue;
    uint2 next=uint2(NP(w,LO),NP(w,HI));if(!present || (op==0?nless(next,value):nless(value,next)))value=next;
    present=true;used=true;NP(w,COMMITTED)=1;
  }
  if(used){if(NS(g,PRESENT)!=uint(present) || any(value!=old))NS(g,VERSION)++;
    NS(g,LO)=value.x;NS(g,HI)=value.y;NS(g,PRESENT)=present;ninc(h,NUMERIC_COUNTERS_REDUCTIONS);}
}
