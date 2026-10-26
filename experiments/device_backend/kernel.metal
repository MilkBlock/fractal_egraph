#include <metal_stdlib>
using namespace metal;
// Generated schema/limits are inserted before this code.
struct Meta {uint count,next_id,epoch,changed,error,matched,rounds,unions,next_fact;};
uint root(device uint*p,uint x){while(p[x]!=x){p[x]=p[p[x]];x=p[x];}return x;}
void unite_now(device uint*p,long a,long b,device Meta&m){uint x=root(p,uint(a)),y=root(p,uint(b));if(x!=y){p[max(x,y)]=min(x,y);m.changed=1;m.unions++;}}
void unite(device uint*p,long a,long b,device Meta&m){
 if(!p[IDCAP]){unite_now(p,a,b,m);return;}
 uint x=root(p,uint(a)),y=root(p,uint(b));if(x==y)return;
 uint n=p[IDCAP+1];if(n>=UNION_CAP){m.error=11;return;}
 p[IDCAP+2+2*n]=x;p[IDCAP+3+2*n]=y;p[IDCAP+1]=n+1;
}
// SET_BODY
long canon(device long*r,device uint*p,uint t,uint c,long x){return EQ[t*COLS+c]==2?set_canon(r,p,x):(EQ[t*COLS+c]==1?long(root(p,uint(x))):x);}
bool keyeq(device long*r,uint row,uint t,thread long*args,device uint*p){
 if(r[row*STRIDE]!=t || r[row*STRIDE+1]==2)return false;
 for(uint c=0;c<WIDTH[t]-1;c++)if(canon(r,p,t,c,r[row*STRIDE+4+c])!=canon(r,p,t,c,args[c]))return false;
 return true;
}
// ROW_INDEX
bool integer_valid(uint op,long a,long b);
long integer_eval(uint op,long a,long b);
__attribute__((noinline)) long ensure_constructor(device long*r,device uint*p,device uint*fp,device Meta&m,uint t,thread long*args){
 if(m.error)return 0;
 long found=row_lookup(r,p,m,t,args);if(found>=0)return canon(r,p,t,WIDTH[t]-1,r[found*STRIDE+4+WIDTH[t]-1]);
 if(m.count>=CAPACITY||m.next_fact>=IDCAP||m.next_id>=IDCAP){m.error=1;return 0;}
 uint value=m.next_id++,i=m.count++,fid=m.next_fact++;p[value]=value;fp[fid]=fid;
 r[i*STRIDE]=t;r[i*STRIDE+1]=0;r[i*STRIDE+2]=m.epoch;r[i*STRIDE+3]=fid;
 for(uint c=0;c<WIDTH[t]-1;c++)r[i*STRIDE+4+c]=canon(r,p,t,c,args[c]);
 r[i*STRIDE+4+WIDTH[t]-1]=value;row_index_insert(r,p,i);m.changed=1;return value;
}
long mergevalue(uint t,long a,long b,device long*r,device uint*p,device uint*fp,device Meta&m){
 if(a==b)return a;
 if(POLICY[t]==3){unite(p,a,b,m);return root(p,uint(a));}
 if(POLICY[t]==4)return a;
 if(POLICY[t]==5)return b;
 if(POLICY[t]==6){switch(t){
 // MERGE_BODY
 }}
 if(POLICY[t]==1)return min(a,b);if(POLICY[t]==2)return max(a,b);
 if(a!=b)m.error=4;return a;
}
void record_query_update(device long*r,uint i){
 if(!QUERY_PATCH){r[QUERY_DIRTY]=1;return;}
 if(r[QUERY_DIRTY]==1||i>=uint(r[QUERY_LOG+1])||r[i*STRIDE+1]!=0)return;
 if(!r[QUERY_OLD_TIME+i]){
  uint n=uint(r[QUERY_LOG]);
  if(n>=CAPACITY){r[QUERY_DIRTY]=1;return;}
  uint t=uint(r[i*STRIDE]);r[QUERY_OLD_VALUE+i]=r[i*STRIDE+4+WIDTH[t]-1];r[QUERY_OLD_TIME+i]=r[i*STRIDE+2];
  r[QUERY_CHANGED+n]=i;r[QUERY_LOG]=n+1;
 }
 r[QUERY_DIRTY]=2;
}
__attribute__((noinline)) long write_row(device long*r,device uint*p,device uint*fp,device Meta&m,uint t,thread long*args,long value,bool create){
 if(m.error)return 0;
 long found=row_lookup(r,p,m,t,args);if(found>=0){uint i=uint(found);
  uint at=i*STRIDE+4+WIDTH[t]-1;long old=canon(r,p,t,WIDTH[t]-1,r[at]);
  if(create)return old;
  long incoming=canon(r,p,t,WIDTH[t]-1,value);
  if(POLICY[t]==4 && r[i*STRIDE+2]==m.epoch && incoming!=old){m.error=7;return old;}
  long next=mergevalue(t,old,incoming,r,p,fp,m);
  if(next!=r[at]){record_query_update(r,i);r[at]=next;r[i*STRIDE+2]=m.epoch;m.changed=1;}return next;
 }
 if(m.count>=CAPACITY || m.next_fact>=IDCAP){m.error=1;return 0;}
 if(create){
  if(KIND[t]==0){if(m.next_id>=IDCAP){m.error=1;return 0;}value=m.next_id;p[m.next_id]=m.next_id;m.next_id++;}
  else if(KIND[t]==1)value=0;
  else {m.error=3;return 0;}
 }
 uint i=m.count++,fid=m.next_fact++;fp[fid]=fid;r[i*STRIDE]=t;r[i*STRIDE+1]=0;r[i*STRIDE+2]=m.epoch;r[i*STRIDE+3]=fid;
 for(uint c=0;c<WIDTH[t]-1;c++)r[i*STRIDE+4+c]=canon(r,p,t,c,args[c]);
 r[i*STRIDE+4+WIDTH[t]-1]=canon(r,p,t,WIDTH[t]-1,value);row_index_insert(r,p,i);m.changed=1;return r[i*STRIDE+4+WIDTH[t]-1];
}
void hide_row(device long*r,device uint*p,device uint*fp,device Meta&m,uint t,thread long*args){
 // Native subsume creates a hidden constructor row when the key is absent.
 write_row(r,p,fp,m,t,args,0,true);if(m.error)return;
 if(INDEXED_SUBSUME&&p[IDCAP]){
  // Deferred unions preserve canonical unique keys throughout the RHS phase.
  long found=row_lookup(r,p,m,t,args);if(found<0){m.error=3;return;}uint i=uint(found);
  if(r[i*STRIDE+1]==0){r[QUERY_DIRTY]=1;r[i*STRIDE+1]=1;r[i*STRIDE+2]=m.epoch;m.changed=1;}
 }else{
  // The legacy eager-union diagnostic can temporarily contain colliding keys.
  for(uint i=0;i<m.count;i++)if(keyeq(r,i,t,args,p)&&r[i*STRIDE+1]==0){r[QUERY_DIRTY]=1;r[i*STRIDE+1]=1;r[i*STRIDE+2]=m.epoch;m.changed=1;}
 }
}
// Query arithmetic rejects a match; failed RHS arithmetic raises an error.
bool integer_valid(uint op,long a,long b){
 const long hi=0x7fffffffffffffffL,lo=-hi-1;
 if(op==0)return !((b>0&&a>hi-b)||(b<0&&a<lo-b));
 if(op==1)return !((b<0&&a>hi+b)||(b>0&&a<lo+b));
 if(op==2){
  if(!a||!b)return true;
  if(a>0)return b>0?a<=hi/b:b>=lo/a;
  return b>0?a>=lo/b:a>=hi/b;
 }
 if(op==3||op==4)return b!=0 && !(a==lo&&b==-1);
 return true;
}
long integer_eval(uint op,long a,long b){
 switch(op){case 0:return a+b;case 1:return a-b;case 2:return a*b;
 case 3:return a/b;case 4:return a%b;case 5:return min(a,b);case 6:return max(a,b);
 case 7:return a==b;case 8:return a<b;case 9:return a>b;case 10:return a<=b;case 11:return a>=b;}
 return 0;
}
__attribute__((noinline)) void rebuild(device long*r,device uint*p,device uint*fp,device Meta&m,device uint*heads,device uint*groups){
 bool again=true;while(again&&!m.error){again=false;
  for(long h=2;h<r[ROW_WORDS];h+=r[ROW_WORDS+h]+4)set_canon(r,p,h);
  for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2){uint t=uint(r[i*STRIDE]);
   for(uint c=0;c<WIDTH[t];c++){uint at=i*STRIDE+4+c;long x=canon(r,p,t,c,r[at]);
    if(x!=r[at]||(EQ[t*COLS+c]==2&&r[ROW_WORDS+x+3]==m.epoch)){r[at]=x;r[i*STRIDE+2]=m.epoch;m.changed=1;}}
  }
  for(uint h=0;h<HASH_CAP;h++)heads[h]=0;
  uint count=m.count,before=m.unions;
  // Freeze duplicate groups before merge effects can change any canonical key.
  for(uint i=0;i<count;i++)if(r[i*STRIDE+1]!=2){uint t=uint(r[i*STRIDE]);long args[COLS]={0};
   for(uint c=0;c<WIDTH[t]-1;c++)args[c]=r[i*STRIDE+4+c];uint h=row_hash(t,args,r,p);
   while(heads[h]&&!keyeq(r,heads[h]-1,t,args,p))h=(h+1)&(HASH_CAP-1);
   if(!heads[h])heads[h]=i+1;groups[i]=heads[h]-1;
  }
  for(uint j=0;j<count&&!m.error;j++)if(r[j*STRIDE+1]!=2&&groups[j]!=j){
   uint i=groups[j],t=uint(r[i*STRIDE]),oi=i*STRIDE+4+WIDTH[t]-1,oj=j*STRIDE+4+WIDTH[t]-1;
   long a=canon(r,p,t,WIDTH[t]-1,r[oi]),b=canon(r,p,t,WIDTH[t]-1,r[oj]);
   if(POLICY[t]==4&&a!=b){m.error=7;return;}
   r[oi]=mergevalue(t,a,b,r,p,fp,m);
   r[i*STRIDE+1]=max(r[i*STRIDE+1],r[j*STRIDE+1]);r[i*STRIDE+2]=m.epoch;r[j*STRIDE+1]=2;
   fp[root(fp,uint(r[j*STRIDE+3]))]=root(fp,uint(r[i*STRIDE+3]));m.changed=1;
   // Writes performed by a later merge must not find a just-retired row.
   r[INDEX_OFFSET]=0;
  }
  again=before!=m.unions||count!=m.count;
 }
 uint n=0;for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2){if(n!=i)for(uint c=0;c<STRIDE;c++)r[n*STRIDE+c]=r[i*STRIDE+c];n++;}m.count=n;r[INDEX_OFFSET]=0;r[QUERY_DIRTY]=1;
}
// QUERY_INDEX
// DEVICE_ACTIONS
kernel void initialize(device uint*p[[buffer(0)]],device uint*fp[[buffer(1)]],uint i[[thread_position_in_grid]]){if(i<IDCAP){p[i]=i;fp[i]=i;}}
#define KERNEL_ARGS device long*r[[buffer(0)]],device long*old[[buffer(1)]],device uint*p[[buffer(2)]],device uint*fp[[buffer(3)]],device Meta&m[[buffer(4)]],device long*matches[[buffer(5)]],device uint*watermarks[[buffer(6)]],constant uint&set[[buffer(7)]],device long*code[[buffer(8)]],device long*registers[[buffer(9)]],device uint*qheads[[buffer(10)]],device uint*qlinks[[buffer(11)]],device uint*control[[buffer(12)]],device uint*match_order[[buffer(15)]],device uint*noop[[buffer(18)]],device uint*qprev[[buffer(19)]],uint lane[[thread_position_in_grid]]
// PARALLEL_REBUILD
// PARALLEL_QUERY
// MATCH_SORT
kernel void prepare(KERNEL_ARGS){
 if(lane||m.error)return;m.changed=0;m.matched=0;control[11]=0;
 if(m.epoch==0xffffffff){m.error=6;return;}m.epoch++;m.rounds++;r[ROW_WORDS+1]=m.epoch;
 bool reset=!INCREMENTAL_QUERY||m.rounds==1||r[QUERY_DIRTY]==1;
 control[0]=reset;control[1]=reset?0:qheads[2*QBUCKETS];
 control[4]=reset?(2*QBUCKETS+255)/256:1;control[5]=1;control[6]=1;
 control[8]=max(1u,(m.count+255)/256);control[9]=1;control[10]=1;
}
kernel void prepare_clear(KERNEL_ARGS){
 if(m.error||!control[0])return;
 if(lane<2*QBUCKETS)qheads[lane]=0xffffffff;
 if(lane<TABLE_COUNT)qheads[2*QBUCKETS+1+lane]=0;
}
kernel void prepare_build(KERNEL_ARGS){
 if(lane||m.error)return;
 // The read phase precedes every RHS write, so old safely aliases r.
 if(!control[0]&&r[QUERY_DIRTY]==2)patch_query_index(old,qheads,qlinks,qprev);
 if(!PARALLEL_PREPARE||!control[0])build_query_index(old,control[1],m.count,false,qheads,qlinks,qprev);
 else qheads[2*QBUCKETS]=m.count;
 for(uint n=0;n<uint(r[QUERY_LOG]);n++)r[QUERY_OLD_TIME+uint(r[QUERY_CHANGED+n])]=0;
 r[QUERY_LOG]=0;r[QUERY_LOG+1]=m.count;
 r[QUERY_DIRTY]=0;
 ensure_row_index(r,p,m);
}
kernel void check_empty_effects(KERNEL_ARGS){
 if(!NOOP_PREFLIGHT||m.error||lane>=m.matched)return;
 // Consume the existing rule-sorted permutation: adjacent lanes execute the
 // same bytecode, rather than diverging across arbitrarily interleaved rules.
 uint binding=m.matched>1?match_order[lane]:lane;
 bool empty=DEFER_UNIONS[set]&&actions_stably_empty(uint(matches[binding*MSTRIDE]),binding,code,matches,r);
 noop[binding]=uint(empty);
 if(empty)atomic_fetch_add_explicit((device atomic_uint*)&control[11],1u,memory_order_relaxed);
}
kernel void commit_effects(KERNEL_ARGS){
 if(lane||m.error)return;uint startUnions=m.unions;
 p[IDCAP]=DEFER_UNIONS[set];p[IDCAP+1]=0;
 for(uint mi=0;mi<m.matched;mi++){
  uint binding=QUERY_PARALLEL[set]&&m.matched>1?match_order[mi]:mi;
  if(NOOP_PREFLIGHT&&QUERY_PARALLEL[set]&&noop[binding])continue;
  uint rule=uint(matches[binding*MSTRIDE]);execute_actions(rule,binding,code,registers,matches,r,p,fp,m);
  if(m.error)return;
 }
 p[IDCAP]=0;for(uint i=0;i<p[IDCAP+1];i++)unite_now(p,p[IDCAP+2+2*i],p[IDCAP+3+2*i],m);
 control[0]=0;control[3]=m.error;
 if(m.unions!=startUnions){if(PARALLEL_REBUILD)control[0]=1;else rebuild(r,p,fp,m,qheads,qlinks);}
 for(uint i=SET_START[set];i<SET_START[set+1];i++)watermarks[SET_RULES[i]]=m.epoch-1;
}
// Correctness-first reference executor. Match against a read-only Run boundary.
kernel void step(KERNEL_ARGS){
 if((!QUERY_PARALLEL[set]&&lane)||atomic_load_explicit((device atomic_uint*)&m.error,memory_order_relaxed))return;uint oldCount=m.count;
 // MATCH_BODY
}
