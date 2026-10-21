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
// Depth 0 is the valid anchor, depth d>0 is the d-th subsequent atom.
// These count work attempts (including retries), never committed mutations.
void depth_work(device atomic_uint* counters,uint q,uint depth,uint field,uint amount=1){
 if(PROFILE_JOIN && amount){
  uint old=atomic_fetch_add_explicit(counters+64+3*NQ+(q*P+depth)*ND+field,amount,memory_order_relaxed);
  if(old>NO-amount)atomic_fetch_or_explicit(counters+1,2,memory_order_relaxed);
 }
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
  depth_work(counters,q,depth,D_KEY_CHECKS);
  if(atomic_load_explicit(heads+index_head(packet,cfg,sp,h),memory_order_relaxed)==NO){
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+43,1,memory_order_relaxed);
   return true;
  }
  if(REFUTATION_SIGNATURES){
   uint id=code[SIGNATURE_SPEC+sp];
   if(id!=NO && (atomic_load_explicit(domains+domain_head(packet,cfg,id,h),memory_order_relaxed)&(1u<<(mixkey(h)&31u)))==0){
    depth_work(counters,q,depth,D_SIGNATURE_REFUTATIONS);
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
  depth_work(counters,q,0,D_DOMAIN_CHECKS);
  if((masks[0]&masks[1])==0){
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+47,1,memory_order_relaxed);
   return true;
  }
 }
 return false;
}
// A conditioned port envelope includes every current native row, plus
// possible bucket collisions and retired rows. Deletions only widen it; rebase
// rebuilds it. Updates/union canonical rows are inserted before any queries.
bool ranges_refuted(device const uint* code,device atomic_uint* domains,device const uint* packet,constant uint* cfg,
 device atomic_uint* counters,uint q,uint anchor,uint depth,thread const uint* v,thread const uint* lo,thread const uint* hi){
 if(!CELL_RANGES)return false;
 uint stage=RANGE_STAGE+((q*P+anchor)*P+depth)*2;
 for(uint i=0;i<code[stage+1];i++){
  uint check=RANGE_CHECK+(code[stage]+i)*9,op=code[check];uint2 ranges[2];
  for(uint side=0;side<2;side++){
   uint at=check+1+4*side,kind=code[at],s=code[at+1];
   if(kind<2){uint x=kind?range_coordinate(s,code[at+2]):range_coordinate(lo[s],hi[s]);ranges[side]=uint2(x);continue;}
   uint t=s,dl=code[at+2],dh=code[at+3],sp=code[DOMAIN_META+2*dl];
   uint spec=SPEC+sp*(2+C),atom=ATOM+(q*P+t)*(2+2*C),h=0x31415926u;
   for(uint j=0;j<code[spec+1];j++){
    uint col=code[spec+2+j],k=code[atom+2+2*col],s=code[atom+3+2*col];
    if(HAS_PRIMITIVES && code[TABLE_I64+code[spec]*C+col]){
     uint lit=ATOM_LITERAL+(q*P+t)*2*C+2*col;
     h=mixkey(h^(k?code[lit]:lo[s]));h=mixkey(h^(k?code[lit+1]:hi[s]));
    }else h=mixkey(h^(k?s:v[s]));
   }
   ranges[side]=uint2(~atomic_load_explicit(domains+domain_head(packet,cfg,dl,h),memory_order_relaxed),atomic_load_explicit(domains+domain_head(packet,cfg,dh,h),memory_order_relaxed));
  }
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+55,1,memory_order_relaxed);
  depth_work(counters,q,depth,D_RANGE_CHECKS);
  uint2 a=ranges[0],b=ranges[1];bool reject=a.x>a.y||b.x>b.y;
  // Only interior coordinates are exact. Equal clipped endpoints never prove
  // an integer equality or disprove a strict comparison.
  bool ab=a.x==b.y && a.x>0 && a.x<NO,ba=b.x==a.y && b.x>0 && b.x<NO;
  reject=reject || (op==0?(a.x>b.y||ab):op==1?(b.x>a.y||ba):op==2?a.x>b.y:op==3?b.x>a.y:
    (a.x==a.y && all(a==b) && a.x>0 && a.x<NO));
  if(reject){
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+56,1,memory_order_relaxed);
   depth_work(counters,q,depth,D_RANGE_REFUTATIONS);return true;
  }
 }
 return false;
}
// A Cell interface is the COMPLETE empty/singleton result of a fully keyed
// fragment, not a list of observed positives. Native commit watches are processed
// before matching, including missing keys. No certificate => ordinary Join.
// We only reject; normal Join retains all timestamps, delta ownership and output.
template<uint QS> bool connection_refuted(device const uint* code,device uint* rows,
 device uint* cells,device uint* rowCells,device uint* instanceMeta,device atomic_uint* counters,
 uint q,uint anchor,uint position,uint r,thread const uint* v,thread const uint* lo,thread const uint* hi,thread const uint* bound){
 if(!CELL_CONNECTIONS)return false;
 uint route=CELL_ROUTE+((q*P+anchor)*P+position)*(2+S),cp=code[route];
 if(cp==NO)return false;
 if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+52,1,memory_order_relaxed);
 depth_work(counters,q,position,D_CONNECTION_CHECKS);
 uint id=cell_lookup(rowCells,instanceMeta,r,cp);
 if(id==NO || cells[id*CS+1]!=R(r,2) || cells[id*CS+4]!=NO || (cells[id*CS]!=1 && cells[id*CS]!=2)){
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+54,1,memory_order_relaxed);
  depth_work(counters,q,position,D_CONNECTION_UNKNOWN);return false;
 }
 uint base=id*CS;bool reject=cells[base]==1;
 if(!reject){
  uint vv[QS],ll[QS],hh[QS],bb[QS];for(uint j=0;j<QS;j++){vv[j]=v[j];ll[j]=lo[j];hh[j]=hi[j];bb[j]=bound[j];}
  for(uint j=0;j<code[CELL_META+cp*6+5];j++){
   uint t=code[route+2+j];if(t==NO)continue;uint at=base+CV+3*j;
   if((bb[t]==1 && vv[t]!=cells[at]) || (bb[t]==2 && (ll[t]!=cells[at+1] || hh[t]!=cells[at+2]))){reject=true;break;}
   vv[t]=cells[at];ll[t]=cells[at+1];hh[t]=cells[at+2];bb[t]=1;
  }
  if(!reject)reject=!primitive_constraints(code,q,vv,ll,hh,bb,false);
  // Non-extended programs keep guards separately. A partial proof may use a
  // guard only when both of its exact integer operands are determined.
  for(uint j=0;j<code[QMETA+q*3+2] && !reject;j++){
   uint at=GUARD+(q*G+j)*7,op=code[at];
   if((!code[at+1]&&!bb[code[at+2]]) || (!code[at+4]&&!bb[code[at+5]]))continue;
   uint2 a=code[at+1]?uint2(code[at+2],code[at+3]):uint2(ll[code[at+2]],hh[code[at+2]]);
   uint2 b=code[at+4]?uint2(code[at+5],code[at+6]):uint2(ll[code[at+5]],hh[code[at+5]]);
   bool lt=nless(a,b),gt=nless(b,a);reject=!(op==0?lt:op==1?gt:op==2?!gt:!lt);
  }
 }
 if(reject){if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+53,1,memory_order_relaxed);depth_work(counters,q,position,D_CONNECTION_REFUTATIONS);}
 return reject;
}
template<uint QP,uint QS> uint2 family_execute_compact(QUERY_ARGS,uint qidx,uint e,uint anchor){
 uint q=packet[4+qidx*4],n=code[QMETA+q*3],cutoff=packet[5+qidx*4];
 if(n==0 || !code[PREADY+q])return family_execute<QP,QS>(QUERY_PASS,qidx,e,anchor,code,0);
 if(e>=cfg[6] || anchor>=n || !live_event(rows,events,e))return uint2(0);
 uint r=E(e,0);if(R(r,3)<cutoff)return uint2(0);
 uint v[QS],lo[QS],hi[QS],bound[QS],cursor[QP],started[QP],hits=0,deepest=0;
 for(uint j=0;j<QS;j++){v[j]=lo[j]=hi[j]=bound[j]=0;}
 if(!bind_atom(code,rows,q,anchor,r,v,lo,hi,bound,false) ||
    !stage_constraints(code,q,anchor,0,v,lo,hi,bound))return uint2(0);
 if(n==1)return uint2(emit_match<QS>(code,output,counters,cfg,q,v,lo,hi,true),0);
 seed_work(counters,q,0);
 depth_work(counters,q,0,D_ENTRIES);depth_work(counters,q,0,D_PREFIXES);
 if(refuted(code,heads,domains,packet,cfg,counters,q,anchor,0,v,lo,hi) ||
    domains_refuted(code,domains,packet,cfg,counters,q,anchor,v,lo,hi) ||
    ranges_refuted(code,domains,packet,cfg,counters,q,anchor,0,v,lo,hi) ||
    connection_refuted<QS>(code,rows,cells,rowCells,instanceMeta,counters,q,anchor,0,r,v,lo,hi,bound)){
  seed_work(counters,q,1);depth_work(counters,q,0,D_REFUTATIONS);depth_work(counters,q,0,D_DEEPEST_SEEDS);return uint2(0);
 }
 for(uint j=0;j<QP;j++)started[j]=0;
 int depth=0;
 while(depth>=0){
  uint plan=PLAN+((q*P+anchor)*(P-1)+uint(depth))*2,t=code[plan],sp=code[plan+1];
  uint atom=ATOM+(q*P+t)*(2+2*C);
  if(!started[depth]){
   if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+2,1,memory_order_relaxed);
   depth_work(counters,q,uint(depth)+1,D_ENTRIES);
   if(PROFILE_JOIN)deepest=max(deepest,uint(depth)+1);
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
  depth_work(counters,q,uint(depth)+1,D_LINK_VISITS);
  cursor[depth]=links[index_link(code,events,next,sp)];if(!live_event(rows,events,next))continue;
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+45,1,memory_order_relaxed);
  depth_work(counters,q,uint(depth)+1,D_LIVE_ROWS);
  uint row=E(next,0);if(precedes(code,q,t,anchor) && R(row,3)>=cutoff)continue;
  uint mask=BSTATE+((q*P+anchor)*P+uint(depth))*BW;
  for(uint j=0;j<QS;j++)bound[j]=(code[mask+j/16]>>(2*(j%16)))&3u;
  if(!bind_atom(code,rows,q,t,row,v,lo,hi,bound,false) ||
     !stage_constraints(code,q,anchor,uint(depth)+1,v,lo,hi,bound))continue;
  depth_work(counters,q,uint(depth)+1,D_PREFIXES);
  if(refuted(code,heads,domains,packet,cfg,counters,q,anchor,uint(depth)+1,v,lo,hi) ||
     ranges_refuted(code,domains,packet,cfg,counters,q,anchor,uint(depth)+1,v,lo,hi) ||
     connection_refuted<QS>(code,rows,cells,rowCells,instanceMeta,counters,q,anchor,uint(depth)+1,row,v,lo,hi,bound)){
   depth_work(counters,q,uint(depth)+1,D_REFUTATIONS);continue;
  }
  if(uint(depth)+1==n-1){uint hit=emit_match<QS>(code,output,counters,cfg,q,v,lo,hi,true);hits+=hit;depth_work(counters,q,uint(depth)+1,D_MATCHED_LEAVES,hit);}
  else{depth++;started[depth]=0;}
 }
 depth_work(counters,q,deepest,D_DEEPEST_SEEDS);
 if(hits)seed_work(counters,q,2);
 return uint2(hits,0);
}
