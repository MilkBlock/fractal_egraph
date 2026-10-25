#include <metal_stdlib>
using namespace metal;
// Generated schema/limits are inserted before this code.
struct Meta {uint count,next_id,epoch,changed,error,matched,rounds,unions,next_fact;};
uint root(device uint*p,uint x){while(p[x]!=x){p[x]=p[p[x]];x=p[x];}return x;}
void unite(device uint*p,long a,long b,device Meta&m){uint x=root(p,uint(a)),y=root(p,uint(b));if(x!=y){p[max(x,y)]=min(x,y);m.changed=1;m.unions++;}}
long canon(device uint*p,uint t,uint c,long x){return EQ[t*COLS+c]?long(root(p,uint(x))):x;}
bool keyeq(device long*r,uint row,uint t,thread long*args,device uint*p){
 if(r[row*STRIDE]!=t || r[row*STRIDE+1]==2)return false;
 for(uint c=0;c<WIDTH[t]-1;c++)if(canon(p,t,c,r[row*STRIDE+4+c])!=canon(p,t,c,args[c]))return false;
 return true;
}
long mergevalue(uint t,long a,long b,device uint*p,device Meta&m){
 if(POLICY[t]==3){unite(p,a,b,m);return root(p,uint(a));}
 if(POLICY[t]==4)return a;
 if(POLICY[t]==1)return min(a,b);if(POLICY[t]==2)return max(a,b);
 if(a!=b)m.error=4;return a;
}
long write_row(device long*r,device uint*p,device uint*fp,device Meta&m,uint t,thread long*args,long value,bool create){
 if(m.error)return 0;
 for(uint i=0;i<m.count;i++)if(keyeq(r,i,t,args,p)){
  uint at=i*STRIDE+4+WIDTH[t]-1;long old=canon(p,t,WIDTH[t]-1,r[at]);
  if(create)return old;
  long incoming=canon(p,t,WIDTH[t]-1,value);
  if(POLICY[t]==4 && r[i*STRIDE+2]==m.epoch && incoming!=old){m.error=7;return old;}
  long next=mergevalue(t,old,incoming,p,m);
  if(next!=r[at]){r[at]=next;r[i*STRIDE+2]=m.epoch;m.changed=1;}return next;
 }
 if(m.count>=CAPACITY || m.next_fact>=IDCAP){m.error=1;return 0;}
 if(create){
  if(KIND[t]==0){if(m.next_id>=IDCAP){m.error=1;return 0;}value=m.next_id;p[m.next_id]=m.next_id;m.next_id++;}
  else if(KIND[t]==1)value=0;
  else {m.error=3;return 0;}
 }
 uint i=m.count++,fid=m.next_fact++;fp[fid]=fid;r[i*STRIDE]=t;r[i*STRIDE+1]=0;r[i*STRIDE+2]=m.epoch;r[i*STRIDE+3]=fid;
 for(uint c=0;c<WIDTH[t]-1;c++)r[i*STRIDE+4+c]=canon(p,t,c,args[c]);
 r[i*STRIDE+4+WIDTH[t]-1]=canon(p,t,WIDTH[t]-1,value);m.changed=1;return r[i*STRIDE+4+WIDTH[t]-1];
}
void hide_row(device long*r,device uint*p,device uint*fp,device Meta&m,uint t,thread long*args){
 // Native subsume creates a hidden constructor row when the key is absent.
 write_row(r,p,fp,m,t,args,0,true);if(m.error)return;
 for(uint i=0;i<m.count;i++)if(keyeq(r,i,t,args,p)&&r[i*STRIDE+1]==0){r[i*STRIDE+1]=1;r[i*STRIDE+2]=m.epoch;m.changed=1;}
}
long add_checked(long a,long b,device Meta&m){
 const long hi=0x7fffffffffffffffL,lo=-hi-1;
 if((b>0&&a>hi-b)||(b<0&&a<lo-b)){m.error=5;return 0;}return a+b;
}
void rebuild(device long*r,device uint*p,device uint*fp,device Meta&m){
 bool again=true;while(again && !m.error){again=false;
  for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2){uint t=uint(r[i*STRIDE]);
   for(uint c=0;c<WIDTH[t];c++){uint at=i*STRIDE+4+c;long x=canon(p,t,c,r[at]);if(x!=r[at]){r[at]=x;r[i*STRIDE+2]=m.epoch;m.changed=1;}}
  }
  for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2){uint t=uint(r[i*STRIDE]);
   for(uint j=i+1;j<m.count;j++)if(r[j*STRIDE+1]!=2&&r[j*STRIDE]==t){
    bool same=true;for(uint c=0;c<WIDTH[t]-1;c++)same=same&&(r[i*STRIDE+4+c]==r[j*STRIDE+4+c]);
    if(!same)continue;
    uint oi=i*STRIDE+4+WIDTH[t]-1,oj=j*STRIDE+4+WIDTH[t]-1,before=m.unions;
    if(POLICY[t]==4 && r[oi]!=r[oj]){m.error=7;return;}
    r[oi]=mergevalue(t,r[oi],r[oj],p,m);again=again||(before!=m.unions);
    // Native egglog combine_subsumed uses max, not logical AND.
    r[i*STRIDE+1]=max(r[i*STRIDE+1],r[j*STRIDE+1]);r[i*STRIDE+2]=m.epoch;r[j*STRIDE+1]=2;
    fp[root(fp,uint(r[j*STRIDE+3]))]=root(fp,uint(r[i*STRIDE+3]));m.changed=1;
   }
  }
 }
 uint n=0;for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2){if(n!=i)for(uint c=0;c<STRIDE;c++)r[n*STRIDE+c]=r[i*STRIDE+c];n++;}m.count=n;
}
kernel void initialize(device uint*p[[buffer(0)]],device uint*fp[[buffer(1)]],uint i[[thread_position_in_grid]]){if(i<IDCAP){p[i]=i;fp[i]=i;}}
// Correctness-first reference executor. A single lane commits deterministic
// actions and rebuilds. It is not a throughput kernel or Tensor Core benchmark.
kernel void step(device long*r[[buffer(0)]],device long*old[[buffer(1)]],device uint*p[[buffer(2)]],device uint*fp[[buffer(3)]],device Meta&m[[buffer(4)]],device long*matches[[buffer(5)]],device uint*watermarks[[buffer(6)]],constant uint&set[[buffer(7)]],uint lane[[thread_position_in_grid]]){
 if(lane||m.error)return;m.changed=0;m.matched=0;uint previous=m.epoch,oldCount=m.count;
 if(m.epoch==0xffffffff){m.error=6;return;}m.epoch++;m.rounds++;
 for(uint i=0;i<oldCount*STRIDE;i++)old[i]=r[i];
 // MATCH_BODY
 if(m.error)return;
 for(uint mi=0;mi<m.matched;mi++){
  uint rule=uint(matches[mi*MSTRIDE]);long v[VARS]={0};for(uint c=0;c<VARS;c++)v[c]=matches[mi*MSTRIDE+1+c];long args[COLS]={0};
  switch(rule){
  // ACTION_BODY
  }
  if(m.error)return;
 }
 rebuild(r,p,fp,m);
 // WATERMARK_BODY
}
