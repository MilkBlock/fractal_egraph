#include <metal_stdlib>
using namespace metal;
// Generated schema/limits are inserted before this code.
struct Meta {uint count,next_id,epoch,changed,error,matched,rounds,unions,next_fact;};
uint root(device uint*p,uint x){while(p[x]!=x){p[x]=p[p[x]];x=p[x];}return x;}
void unite(device uint*p,long a,long b,device Meta&m){uint x=root(p,uint(a)),y=root(p,uint(b));if(x!=y){p[max(x,y)]=min(x,y);m.changed=1;m.unions++;}}
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
long ensure_constructor(device long*r,device uint*p,device uint*fp,device Meta&m,uint t,thread long*args){
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
long write_row(device long*r,device uint*p,device uint*fp,device Meta&m,uint t,thread long*args,long value,bool create){
 if(m.error)return 0;
 long found=row_lookup(r,p,m,t,args);if(found>=0){uint i=uint(found);
  uint at=i*STRIDE+4+WIDTH[t]-1;long old=canon(r,p,t,WIDTH[t]-1,r[at]);
  if(create)return old;
  long incoming=canon(r,p,t,WIDTH[t]-1,value);
  if(POLICY[t]==4 && r[i*STRIDE+2]==m.epoch && incoming!=old){m.error=7;return old;}
  long next=mergevalue(t,old,incoming,r,p,fp,m);
  if(next!=r[at]){r[at]=next;r[i*STRIDE+2]=m.epoch;m.changed=1;}return next;
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
 for(uint i=0;i<m.count;i++)if(keyeq(r,i,t,args,p)&&r[i*STRIDE+1]==0){r[i*STRIDE+1]=1;r[i*STRIDE+2]=m.epoch;m.changed=1;}
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
void rebuild(device long*r,device uint*p,device uint*fp,device Meta&m){
 bool again=true;while(again && !m.error){again=false;
  for(long h=2;h<r[ROW_WORDS];h+=r[ROW_WORDS+h]+4)set_canon(r,p,h);
  for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2){uint t=uint(r[i*STRIDE]);
   for(uint c=0;c<WIDTH[t];c++){uint at=i*STRIDE+4+c;long x=canon(r,p,t,c,r[at]);if(x!=r[at]||(EQ[t*COLS+c]==2 && r[ROW_WORDS+x+3]==m.epoch)){r[at]=x;r[i*STRIDE+2]=m.epoch;m.changed=1;}}
  }
  for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2){uint t=uint(r[i*STRIDE]);
   for(uint j=i+1;j<m.count;j++)if(r[j*STRIDE+1]!=2&&r[j*STRIDE]==t){
    bool same=true;for(uint c=0;c<WIDTH[t]-1;c++)same=same&&(r[i*STRIDE+4+c]==r[j*STRIDE+4+c]);
    if(!same)continue;
    uint oi=i*STRIDE+4+WIDTH[t]-1,oj=j*STRIDE+4+WIDTH[t]-1,before=m.unions;
    if(POLICY[t]==4 && r[oi]!=r[oj]){m.error=7;return;}
    r[oi]=mergevalue(t,r[oi],r[oj],r,p,fp,m);again=again||(before!=m.unions);
    // Native egglog combine_subsumed uses max, not logical AND.
    r[i*STRIDE+1]=max(r[i*STRIDE+1],r[j*STRIDE+1]);r[i*STRIDE+2]=m.epoch;r[j*STRIDE+1]=2;
    fp[root(fp,uint(r[j*STRIDE+3]))]=root(fp,uint(r[i*STRIDE+3]));m.changed=1;
   }
  }
 }
 uint n=0;for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2){if(n!=i)for(uint c=0;c<STRIDE;c++)r[n*STRIDE+c]=r[i*STRIDE+c];n++;}m.count=n;r[INDEX_OFFSET]=0;
}
// DEVICE_ACTIONS
kernel void initialize(device uint*p[[buffer(0)]],device uint*fp[[buffer(1)]],uint i[[thread_position_in_grid]]){if(i<IDCAP){p[i]=i;fp[i]=i;}}
// Correctness-first reference executor. A single lane commits deterministic
// actions and rebuilds. It is not a throughput kernel or Tensor Core benchmark.
kernel void step(device long*r[[buffer(0)]],device long*old[[buffer(1)]],device uint*p[[buffer(2)]],device uint*fp[[buffer(3)]],device Meta&m[[buffer(4)]],device long*matches[[buffer(5)]],device uint*watermarks[[buffer(6)]],constant uint&set[[buffer(7)]],device long*code[[buffer(8)]],device long*registers[[buffer(9)]],uint lane[[thread_position_in_grid]]){
 if(lane||m.error)return;m.changed=0;m.matched=0;uint previous=m.epoch,oldCount=m.count,startUnions=m.unions;
 if(m.epoch==0xffffffff){m.error=6;return;}m.epoch++;m.rounds++;r[ROW_WORDS+1]=m.epoch;
 for(uint i=0;i<oldCount*STRIDE;i++)old[i]=r[i];
 // MATCH_BODY
 if(m.error)return;
 for(uint mi=0;mi<m.matched;mi++){
  uint rule=uint(matches[mi*MSTRIDE]);execute_actions(rule,mi,code,registers,matches,r,p,fp,m);
  if(m.error)return;
 }
 if(m.unions!=startUnions)rebuild(r,p,fp,m);
 // WATERMARK_BODY
}
