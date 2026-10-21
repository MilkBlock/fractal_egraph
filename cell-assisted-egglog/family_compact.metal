// One binding per search lane. A child may only assign previously unbound
// slots (or attach the native ID to an already computed integer). It cannot
// overwrite an ancestor value: bind_atom/primitive_apply check it first.
// Backtracking therefore restores bound-status masks, not copies of values.
// This path has no Cell segment jumps; those retain the general executor.
// Profile multi-atom starts only. Keep a real denominator, and retain rule
// identity so removing easy queries cannot masquerade as stronger proof.
void seed_work(device atomic_uint* counters,uint q,uint kind){
 if(PROFILE_JOIN){atomic_fetch_add_explicit(counters+49+kind,1,memory_order_relaxed);atomic_fetch_add_explicit(counters+64+q*3+kind,1,memory_order_relaxed);}
}
bool refuted(device const uint* code,device atomic_uint* heads,device atomic_uint* domains,device const uint* packet,
 constant uint* cfg,device atomic_uint* counters,uint q,uint anchor,uint depth,
 thread const uint* v,thread const uint* lo,thread const uint* hi){
 if(!REFUTATION)return false;
 uint stage=REFUTE_STAGE+((q*P+anchor)*P+depth)*2;
 for(uint i=0;i<code[stage+1];i++){
  uint check=REFUTE_CHECK+(code[stage]+i)*2,t=code[check],sp=code[check+1];
  uint at=SPEC+sp*(2+C),atom=ATOM+(q*P+t)*(2+2*C),h=0x31415926u;
  for(uint j=0;j<code[at+1];j++){
   uint col=code[at+2+j],kind=code[atom+2+2*col],s=code[atom+3+2*col];
   if(HAS_PRIMITIVES && code[TABLE_I64+code[at]*C+col]){
    uint lit=ATOM_LITERAL+(q*P+t)*2*C+2*col;
    h=mixkey(h^(kind?code[lit]:lo[s]));h=mixkey(h^(kind?code[lit+1]:hi[s]));
   }else h=mixkey(h^(kind?s:v[s]));
  }
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+42,1,memory_order_relaxed);
  if(atomic_load_explicit(heads+index_head(packet,cfg,sp,h),memory_order_relaxed)==NO){
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+43,1,memory_order_relaxed);
   return true;
  }
  if(REFUTATION_SIGNATURES){
   uint id=code[SIGNATURE_SPEC+sp];
   if(id!=NO && (atomic_load_explicit(domains+domain_head(packet,cfg,id,h),memory_order_relaxed)&(1u<<(mixkey(h)&31u)))==0){
    if(PROFILE_JOIN){atomic_fetch_add_explicit(counters+48,1,memory_order_relaxed);atomic_fetch_add_explicit(counters+43,1,memory_order_relaxed);}
    return true;
   }
  }
 }
 return false;
}
// Bucket collisions and retired rows only enlarge these value sets. A real
// shared value sets the same bit on both sides, so zero intersection is sound.
bool domains_refuted(device const uint* code,device atomic_uint* domains,device const uint* packet,
 constant uint* cfg,device atomic_uint* counters,uint q,uint anchor,
 thread const uint* v,thread const uint* lo,thread const uint* hi){
 if(!REFUTATION_DOMAINS)return false;
 uint stage=DOMAIN_STAGE+(q*P+anchor)*2;
 for(uint i=0;i<code[stage+1];i++){
  uint check=DOMAIN_CHECK+(code[stage]+i)*4,masks[2];
  for(uint side=0;side<2;side++){
   uint t=code[check+2*side],id=code[check+2*side+1],sp=code[DOMAIN_META+2*id];
   uint at=SPEC+sp*(2+C),atom=ATOM+(q*P+t)*(2+2*C),h=0x31415926u;
   for(uint j=0;j<code[at+1];j++){
    uint col=code[at+2+j],kind=code[atom+2+2*col],s=code[atom+3+2*col];
    if(HAS_PRIMITIVES && code[TABLE_I64+code[at]*C+col]){
     uint lit=ATOM_LITERAL+(q*P+t)*2*C+2*col;
     h=mixkey(h^(kind?code[lit]:lo[s]));h=mixkey(h^(kind?code[lit+1]:hi[s]));
    }else h=mixkey(h^(kind?s:v[s]));
   }
   masks[side]=atomic_load_explicit(domains+domain_head(packet,cfg,id,h),memory_order_relaxed);
  }
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+46,1,memory_order_relaxed);
  if((masks[0]&masks[1])==0){
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+47,1,memory_order_relaxed);
   return true;
  }
 }
 return false;
}
template<uint QP,uint QS> uint2 family_execute_compact(QUERY_ARGS,uint qidx,uint e,uint anchor){
 uint q=packet[4+qidx*4],n=code[QMETA+q*3],cutoff=packet[5+qidx*4];
 if(n==0 || !code[PREADY+q])return family_execute<QP,QS>(QUERY_PASS,qidx,e,anchor,code,0);
 if(e>=cfg[6] || anchor>=n || !live_event(rows,events,e))return uint2(0);
 uint r=E(e,0);if(R(r,3)<cutoff)return uint2(0);
 uint v[QS],lo[QS],hi[QS],bound[QS],cursor[QP],started[QP],hits=0;
 for(uint j=0;j<QS;j++){v[j]=lo[j]=hi[j]=bound[j]=0;}
 if(!bind_atom(code,rows,q,anchor,r,v,lo,hi,bound,false) ||
    !stage_constraints(code,q,anchor,0,v,lo,hi,bound))return uint2(0);
 if(n==1)return uint2(emit_match<QS>(code,output,counters,cfg,q,v,lo,hi,true),0);
 seed_work(counters,q,0);
 if(refuted(code,heads,domains,packet,cfg,counters,q,anchor,0,v,lo,hi) ||
    domains_refuted(code,domains,packet,cfg,counters,q,anchor,v,lo,hi)){
  seed_work(counters,q,1);return uint2(0);
 }
 for(uint j=0;j<QP;j++)started[j]=0;
 int depth=0;
 while(depth>=0){
  uint plan=PLAN+((q*P+anchor)*(P-1)+uint(depth))*2,t=code[plan],sp=code[plan+1];
  uint atom=ATOM+(q*P+t)*(2+2*C);
  if(!started[depth]){
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+2,1,memory_order_relaxed);
   uint at=SPEC+sp*(2+C),h=0x31415926u;
   for(uint j=0;j<code[at+1];j++){
    uint col=code[at+2+j],kind=code[atom+2+2*col],s=code[atom+3+2*col];
    if(HAS_PRIMITIVES && code[TABLE_I64+code[at]*C+col]){
     uint lit=ATOM_LITERAL+(q*P+t)*2*C+2*col;
     h=mixkey(h^(kind?code[lit]:lo[s]));h=mixkey(h^(kind?code[lit+1]:hi[s]));
    }else h=mixkey(h^(kind?s:v[s]));
   }
   cursor[depth]=atomic_load_explicit(heads+index_head(packet,cfg,sp,h),memory_order_relaxed);started[depth]=1;
  }
  uint next=cursor[depth];if(next==NO){started[depth]=0;depth--;continue;}
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+44,1,memory_order_relaxed);
  cursor[depth]=links[index_link(code,events,next,sp)];if(!live_event(rows,events,next))continue;
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+45,1,memory_order_relaxed);
  uint row=E(next,0);if(precedes(code,q,t,anchor) && R(row,3)>=cutoff)continue;
  uint mask=BSTATE+((q*P+anchor)*P+uint(depth))*BW;
  for(uint j=0;j<QS;j++)bound[j]=(code[mask+j/16]>>(2*(j%16)))&3u;
  if(!bind_atom(code,rows,q,t,row,v,lo,hi,bound,false) ||
     !stage_constraints(code,q,anchor,uint(depth)+1,v,lo,hi,bound))continue;
  if(refuted(code,heads,domains,packet,cfg,counters,q,anchor,uint(depth)+1,v,lo,hi))continue;
  if(uint(depth)+1==n-1)hits+=emit_match<QS>(code,output,counters,cfg,q,v,lo,hi,true);
  else{depth++;started[depth]=0;}
 }
 if(hits)seed_work(counters,q,2);
 return uint2(hits,0);
}
