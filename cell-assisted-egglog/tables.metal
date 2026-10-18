// The delta-anchor ownership and tuple-index traversal follow resident.metal.
// Data dimensions are uniforms; the rule-shaped kernel is compiled only once.
#define NO 0xffffffffu
#define RS (3+SLOTS+2*NUMS)
#define MS (4+PARTS)
#define CTX device const uint* code [[buffer(0)]], device uint* rows [[buffer(1)]], device uint* links [[buffer(2)]], device atomic_uint* heads [[buffer(3)]], device uint* matches [[buffer(4)]], device uint* scratch [[buffer(5)]], device uint* groups [[buffer(6)]], device atomic_uint* meta [[buffer(7)]], constant uint* cfg [[buffer(8)]], device const uint* patch [[buffer(9)]]
#define ROW(i,f) rows[(i)*RS+(f)]
#define MAT(i,f) matches[(i)*MS+(f)]
#define MLOAD(i) atomic_load_explicit(meta+(i),memory_order_relaxed)
#define INC(i) atomic_fetch_add_explicit(meta+(i),1,memory_order_relaxed)
uint tmix(uint x){x^=x>>16;x*=0x7feb352du;x^=x>>15;x*=0x846ca68bu;return x^(x>>16);}
uint thash(device const uint* code,uint spec,thread const uint* v){
  uint k=INDEX+spec*(SLOTS+2),h=0x29a95d53u;
  for(uint j=0;j<code[k+1];j++)h=tmix(h^v[code[k+2+j]]);return h;
}
kernel void table_patch(CTX,uint i [[thread_position_in_grid]]){
  if(i<cfg[2]){
    uint row=cfg[1]+i,at=4+i*(1+SLOTS+2*NUMS);
    ROW(row,0)=patch[at];ROW(row,1)=1;ROW(row,2)=cfg[3];
    for(uint j=0;j<SLOTS+2*NUMS;j++)ROW(row,3+j)=patch[at+1+j];
  }
  if(i<patch[2])ROW(patch[4+cfg[2]*(1+SLOTS+2*NUMS)+i],1)=0;
}
kernel void table_clear_indexes(CTX,uint i [[thread_position_in_grid]]){
  if(i<cfg[4]*NINDEX)atomic_store_explicit(heads+i,NO,memory_order_relaxed);
}
kernel void table_index(CTX,uint i [[thread_position_in_grid]]){
  if(i>=cfg[0] || !ROW(i,1) || (cfg[9]==0 && i<cfg[1]))return;
  uint v[SLOTS];for(uint s=0;s<SLOTS;s++)v[s]=ROW(i,3+s);
  for(uint spec=0;spec<NINDEX;spec++)if(code[INDEX+spec*(SLOTS+2)]==ROW(i,0)){
    uint bucket=spec*cfg[4]+(thash(code,spec,v)&(cfg[4]-1));
    links[i*max(1,NINDEX)+spec]=atomic_exchange_explicit(heads+bucket,i,memory_order_relaxed);
  }
}
void temit(device const uint*code,device uint*rows,device uint*matches,device atomic_uint*meta,constant uint*cfg,thread const uint*src){
  uint2 values[EXPRS];bool defined[EXPRS];
  for(uint e=0;e<EXPRS;e++){
    uint at=EXPR+6*e,op=code[at],slot=code[at+1],a=code[at+2],b=code[at+3];bool ok=true;uint2 v;
    if(op==0){uint r=src[code[NUM_OWNERS+slot]];v=uint2(ROW(r,3+SLOTS+slot*2),ROW(r,4+SLOTS+slot*2));}
    else if(op==2)v=uint2(code[at+4],code[at+5]);
    else {
      uint2 x=values[a],y=values[b];
      if(op<=5){uint3 q=op==3?nadd(x,y):op==4?nsub(x,y):nmul(x,y);v=q.xy;ok=q.z!=0;}
      else v=op==6?(nless(x,y)?x:y):(nless(x,y)?y:x);
      ok&=defined[a] && defined[b];
    }
    values[e]=v;defined[e]=ok;
  }
  for(uint g=0;g<NGUARDS;g++){
    uint at=GUARDS+g*3,a=code[at],b=code[at+1],op=code[at+2];uint2 x=values[a],y=values[b];
    bool lt=nless(x,y),gt=nless(y,x),eq=all(x==y);
    if(!defined[a] || !defined[b] || !(op==0?lt:op==1?!gt:op==2?gt:op==3?!lt:op==4?eq:!eq))return;
  }
  uint i=INC(0);
  if(i==NO)atomic_fetch_or_explicit(meta+1,2,memory_order_relaxed);
  if(i>=cfg[5]){atomic_fetch_or_explicit(meta+1,1,memory_order_relaxed);return;}
  MAT(i,0)=1;MAT(i,1)=defined[RHS];MAT(i,2)=values[RHS].x;MAT(i,3)=values[RHS].y;
  for(uint p=0;p<PARTS;p++)MAT(i,4+p)=src[p];
}
kernel void table_join(CTX,uint index [[thread_position_in_grid]]){
  if(index>=cfg[2])return;
  uint i=cfg[1]+index;if(!ROW(i,1))return;
  uint anchor=ROW(i,0),frames[PARTS][SLOTS],cursor[PARTS],started[PARTS],src[PARTS];
  for(uint s=0;s<SLOTS;s++)frames[0][s]=ROW(i,3+s);
  for(uint p=0;p<PARTS;p++)started[p]=0;src[anchor]=i;
  if(PARTS==1){temit(code,rows,matches,meta,cfg,src);return;}
  int depth=0;
  while(depth>=0){
    uint spec=code[PLANS+anchor*(PARTS-1)+uint(depth)],part=code[INDEX+spec*(SLOTS+2)];
    if(!started[depth]){
      cursor[depth]=atomic_load_explicit(heads+spec*cfg[4]+(thash(code,spec,frames[depth])&(cfg[4]-1)),memory_order_relaxed);
      started[depth]=1;INC(5);
    }
    uint row=cursor[depth];
    if(row==NO){started[depth]=0;depth--;continue;}
    cursor[depth]=links[row*max(1,NINDEX)+spec];INC(4);
    if(!ROW(row,1) || (part<anchor && ROW(row,2)==cfg[3]))continue;
    bool valid=true;
    for(uint s=0;s<SLOTS;s++)if(frames[depth][s] && ROW(row,3+s) && frames[depth][s]!=ROW(row,3+s))valid=false;
    if(!valid)continue;
    src[part]=row;
    for(uint s=0;s<SLOTS;s++)frames[depth+1][s]=frames[depth][s]?frames[depth][s]:ROW(row,3+s);
    if(depth==PARTS-2)temit(code,rows,matches,meta,cfg,src);
    else{depth++;started[depth]=0;}
  }
}
uint tkey(device const uint*code,device uint*rows,device uint*matches,uint m,uint key){
  uint at=KEYS+key*3;
  return code[at]==1?code[at+1]:ROW(MAT(m,4+code[at+2]),3+code[at+1]);
}
kernel void table_pending(CTX,uint i [[thread_position_in_grid]]){
  if(i>=cfg[7])return;
  bool live=true;for(uint p=0;p<PARTS;p++)live&=ROW(MAT(i,4+p),1)!=0;
  scratch[2*cfg[5]+i]=live;scratch[cfg[5]+i]=0;
  if(live){INC(7);if(MAT(i,0)){if(!MAT(i,1))atomic_store_explicit(meta+2,1,memory_order_relaxed);else INC(6);}}
}
kernel void table_clear_groups(CTX,uint i [[thread_position_in_grid]]){
  if(i<cfg[8])scratch[3*cfg[5]+i]=NO;
}
kernel void table_group_links(CTX,uint i [[thread_position_in_grid]]){
  if(i>=cfg[7] || MLOAD(2) || !scratch[2*cfg[5]+i] || !MAT(i,0) || !MAT(i,1))return;
  uint h=0x31415926u;for(uint k=0;k<NKEYS;k++)h=tmix(h^tkey(code,rows,matches,i,k));
  scratch[i]=atomic_exchange_explicit((device atomic_uint*)(scratch+3*cfg[5]+(h&(cfg[8]-1))),i,memory_order_relaxed);
}
kernel void table_reduce(CTX,uint b [[thread_position_in_grid]]){
  if(b>=cfg[8] || MLOAD(2))return;
  for(uint i=scratch[3*cfg[5]+b];i!=NO;i=scratch[i]){
    if(scratch[cfg[5]+i])continue;
    uint2 value=uint2(MAT(i,2),MAT(i,3));
    for(uint j=i;j!=NO;j=scratch[j]){
      bool same=true;for(uint k=0;k<NKEYS;k++)same&=tkey(code,rows,matches,i,k)==tkey(code,rows,matches,j,k);
      if(same){scratch[cfg[5]+j]=1;uint2 next=uint2(MAT(j,2),MAT(j,3));if(MIN_MERGE?nless(next,value):nless(value,next))value=next;}
    }
    uint g=INC(3);groups[g*3]=i;groups[g*3+1]=value.x;groups[g*3+2]=value.y;
  }
}
kernel void table_acknowledge(CTX,uint i [[thread_position_in_grid]]){
  if(i<cfg[7] && scratch[2*cfg[5]+i] && MAT(i,1))MAT(i,0)=0;
}
