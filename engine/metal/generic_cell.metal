// All evolving storage is private to one Cell instance, including identities.
// A Run collects *all* bindings before invoking any action. The following Run
// sees locally rebuilt results without global publication or CPU coordination.
kernel void generic_cell_advance(
 device long*all_rows[[buffer(0)]], device uint*all_uint[[buffer(1)]],
 device long*all_long[[buffer(2)]], device Meta*all_meta[[buffer(3)]],
 device uint*all_state[[buffer(4)]], device long*code[[buffer(5)]],
 device const uint*tape[[buffer(6)]], device uint*all_history[[buffer(7)]],
 device long*all_markers[[buffer(8)]], constant uint4&cfg[[buffer(9)]], uint instance[[thread_position_in_grid]]) {
 if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device uint*s=all_state+instance*GC_STATE_WORDS;
 if(s[2]||m.error)return;
 device uint*stack=s+8;device long*r=all_rows+instance*GC_ROW_WORDS;device long*old=r;
 // LOCAL_POINTERS
 device uint*p=parent;
 // No split/ridge queries are used in this interpreter; this slot is unused.
 device float*context_results=(device float*)contexts;
 for(uint work=0;work<cfg.y&&!m.error&&!s[2];){
  uint pc=s[0],op=tape[pc*4],arg=tape[pc*4+1];
  if(op==1||op==2){
   if(op==1&&!arg){s[0]=tape[pc*4+2];continue;}
   uint at=8*s[1]++;stack[at]=op;stack[at+1]=arg;stack[at+2]=pc+1;stack[at+3]=tape[pc*4+2];
   for(uint j=4;j<8;j++)stack[at+j]=0;s[0]++;continue;
  }
  if(op==3){
   if(!s[1]){m.error=20;return;}uint at=8*(s[1]-1),changed=stack[at+5];
   stack[at+4]++;stack[at+6]|=changed;
   if(changed&&(stack[at]==2||stack[at+4]<stack[at+1])){stack[at+5]=0;s[0]=stack[at+2];}
   else{s[0]=stack[at+3];s[1]--;if(s[1])stack[8*(s[1]-1)+5]|=stack[at+6];}continue;
  }
  if(op==4){s[2]=1;return;}
  if(op==5){bool found=false;for(uint i=0;i<m.count;i++)found|=r[i*STRIDE]==arg&&r[i*STRIDE+1]==0;
   if(!found){m.error=8;return;}s[0]++;continue;}
  if(op!=0){m.error=20;return;}
  // Yield before a Run when history needs growing. No inference is dropped.
  if(s[3]>=cfg.z){s[4]=1;return;}
  if(!s[7]&&m.epoch==0xffffffff){m.error=6;return;}
  uint set=arg,startUnions=m.unions;
  if(!s[7]){m.epoch++;m.rounds++;m.changed=0;}
  m.matched=0;
  r[ROW_WORDS+1]=m.epoch;
  bool reset=m.rounds==1||r[QUERY_DIRTY]==1;
  build_query_index(old,reset?0:qheads[2*QBUCKETS],m.count,reset,qheads,qlinks,qprev);
  if(reset)s[5]++;else s[6]++;
  r[QUERY_LOG]=0;r[QUERY_LOG+1]=m.count;r[QUERY_DIRTY]=0;ensure_row_index(r,p,m);
#if RC_ENABLED
  if(m.rounds==1)for(uint row=0;row<m.count;row++)r[RC_FACT_ROW+uint(r[row*STRIDE+3])]=row+1;
  rc_compact(contexts,r);
#endif
  s[7]=1; // Retry a cache-allocation failure at this same frozen Run, before RHS.
  switch(set){// LOCAL_QUERIES
   default:m.error=20;return;
  }
  if(m.error)return;
  s[7]=0;
  p[IDCAP]=1;p[IDCAP+1]=0;
  for(uint binding=0;binding<m.matched&&!m.error;binding++)
   execute_actions(uint(matches[binding*MSTRIDE]),binding,code,registers,matches,r,p,fp,m);
  if(m.error)return;
  p[IDCAP]=0;for(uint j=0;j<p[IDCAP+1];j++)unite_now(p,p[IDCAP+2+2*j],p[IDCAP+3+2*j],m);
  if(m.unions!=startUnions)rebuild(r,p,fp,m,qheads,qlinks);
  if(m.error)return;
  for(uint j=SET_START[set];j<SET_START[set+1];j++)watermarks[SET_RULES[j]]=m.epoch-1;
  device uint*h=all_history+(instance*cfg.z+s[3])*8;
  h[0]=set;h[1]=m.changed;h[2]=m.matched;h[3]=m.epoch;h[4]=m.count;h[5]=m.unions;h[6]=pc;h[7]=s[1];
  s[3]++;s[0]++;work++;if(s[1])stack[8*(s[1]-1)+5]|=m.changed;
 }
}
