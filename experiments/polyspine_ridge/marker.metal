#include <metal_stdlib>
#include <metal_simdgroup_matrix>
using namespace metal;
long checked(long a,long b,uint op,thread bool &ok){ulong ua=as_type<ulong>(a),ub=as_type<ulong>(b),u=op?ua-ub:ua+ub;ulong ov=op?((ua^ub)&(ua^u)):((~(ua^ub))&(ua^u));if(ov>>63)ok=false;return as_type<long>(u);}
// COMPILED_PROGRAM
// cfg: markers, slots, queries, matrix width, affine recipe count
kernel void gather(device const uint*r,device const long*s,device half*x,constant uint*cfg,uint i[[thread_position_in_grid]]){
 if(i>=cfg[0])return;uint base=i*6,rid=r[base+1],arity=r[base+5];bool ready=true,safe=true;long costs[3]={0,0,0};
 for(uint j=0;j<arity;j++){uint p=r[base+2+j];ready=ready&&s[p*4+1];costs[j]=s[p*4];safe=safe&&costs[j]>=0&&costs[j]<=2048;}
 for(uint j=0;j<cfg[3];j++)x[i*cfg[3]+j]=0;
 if(rid<cfg[4]&&ready&&safe){for(uint j=0;j<3;j++)x[i*cfg[3]+rid*4+j]=half(costs[j]);x[i*cfg[3]+rid*4+3]=1;}
}
kernel void mma(device const half*x,device const half*w,device float*y,constant uint*cfg,uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 uint row=tile*8;threadgroup half a[64],b[64];threadgroup float ans[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<cfg[3];k+=8){for(uint z=lane;z<64;z+=32){uint rr=z/8,c=z%8;a[z]=row+rr<cfg[0]?x[(row+rr)*cfg[3]+k+c]:half(0);b[z]=w[(k+rr)*8+c];}
 threadgroup_barrier(mem_flags::mem_threadgroup);simdgroup_half8x8 aa,bb;simdgroup_load(aa,a,8);simdgroup_load(bb,b,8);simdgroup_multiply_accumulate(acc,aa,bb,acc);threadgroup_barrier(mem_flags::mem_threadgroup);}
 simdgroup_store(acc,ans,8);threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint z=lane;z<64;z+=32){uint rr=row+z/8,c=z%8;if(rr<cfg[0]&&c==0)y[rr]=ans[z];}
}
kernel void candidate(device const uint*r,device const long*s,device const float*y,device long*out,constant uint*cfg,constant long&initializer,uint i[[thread_position_in_grid]]){
 if(i>=cfg[0])return;uint b=i*6,rid=r[b+1],arity=r[b+5];bool ok=true,safe=true;long v[16]={0},value=0;
 for(uint j=0;j<arity;j++){uint p=r[b+2+j];ok=ok&&s[p*4+1];v[j]=s[p*4];safe=safe&&v[j]>=0&&v[j]<=2048;}
 uint mismatch=0;
 if(ok){if(rid==cfg[4])value=v[0];else if(rid==cfg[4]+1)value=min(v[0],v[1]);else{
 evaluate(rid,v,ok,value);if(ok&&safe){mismatch=long(y[i])!=value;value=long(y[i]);}if(ok)value=min(value,initializer);
 }}
 out[i*3]=ok?value:0;out[i*3+1]=ok;out[i*3+2]=mismatch;
}
kernel void reset(device atomic_uint*changed,uint i[[thread_position_in_grid]]){atomic_store_explicit(changed,0,memory_order_relaxed);}
kernel void reduce(device const long*old,device long*next,device const long*c,device const uint*offset,device const uint*index,device atomic_uint*changed,constant uint*cfg,uint i[[thread_position_in_grid]]){
 if(i>=cfg[1])return;long cost=old[i*4],ready=old[i*4+1],producer=old[i*4+2];
 for(uint q=offset[i];q<offset[i+1];q++){uint k=index[q];if(c[k*3+1]&&(!ready||c[k*3]<cost)){cost=c[k*3];ready=1;producer=k;}}
 bool diff=cost!=old[i*4]||ready!=old[i*4+1];next[i*4]=cost;next[i*4+1]=ready;next[i*4+2]=producer;next[i*4+3]=old[i*4+3]+diff;
 if(diff)atomic_store_explicit(changed,1,memory_order_relaxed);
}
float logcost(long x){return log2(1.0f+float(x))/64;}
kernel void features(device const uint*r,device const long*initial,device const long*now,device const long*old,device const long*c,device const uint*offset,device const uint*index,device const float*stat,device float*f,constant uint*cfg,uint root[[thread_position_in_grid]]){
 if(root>=cfg[2])return;float v[24]={0};uint start=offset[root],end=offset[root+1],valid=0,arity=0,ready=0,newly=0,improved=0;float lo=INFINITY,hi=0,sum=0,best=0;
 long before=initial[root*4],after=now[root*4];bool had=initial[root*4+1],has=now[root*4+1];
 for(uint q=start;q<end;q++){uint i=index[q],n=r[i*6+5],nr=0;arity+=n;
  for(uint j=0;j<n;j++){uint p=r[i*6+2+j];nr+=uint(now[p*4+1]);newly+=now[p*4+1]&&!initial[p*4+1];improved+=now[p*4+1]&&initial[p*4+1]&&now[p*4]<initial[p*4];}
  ready+=nr;best=max(best,n?float(nr)/float(n):1.0f);
  if(c[i*3+1]){valid++;float z=logcost(c[i*3]);lo=min(lo,z);hi=max(hi,z);sum+=z;if(had){v[13]=max(v[13],float(c[i*3]<before));v[14]=max(v[14],float(c[i*3]==before));v[15]=max(v[15],float(c[i*3]>before));}}
 }
 v[0]=log2(1.0f+float(end-start))/16;v[1]=had;v[2]=had?logcost(before):0;v[3]=has;v[4]=has?logcost(after):0;v[5]=has&&!had;v[6]=has&&had&&after<before;v[7]=min(1.0f,float(now[root*4+3])/16);
 v[8]=float(valid)/max(1.0f,float(end-start));v[9]=log2(1.0f+float(valid))/16;v[10]=valid?lo:0;v[11]=valid?hi:0;v[12]=valid?sum/float(valid):0;
 v[16]=after!=old[root*4]||has!=bool(old[root*4+1]);v[17]=log2(1.0f+stat[root*4])/16;v[18]=log2(1.0f+stat[root*4+1])/16;v[19]=float(ready)/max(1.0f,float(arity));v[20]=float(newly)/max(1.0f,float(arity));v[21]=float(improved)/max(1.0f,float(arity));v[22]=best;v[23]=log2(1.0f+stat[root*4+3])/16;
 for(uint j=0;j<24;j++)f[root*24+j]=v[j];
}
