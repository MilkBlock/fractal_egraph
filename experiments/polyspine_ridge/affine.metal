#include <metal_stdlib>
#include <metal_simdgroup_matrix>
using namespace metal;
long checked(long a,long b,uint op,thread bool &ok){
 ulong ua=as_type<ulong>(a),ub=as_type<ulong>(b),u=op?ua-ub:ua+ub;
 ulong overflow=op?((ua^ub)&(ua^u)):((~(ua^ub))&(ua^u));
 if(overflow>>63)ok=false;return as_type<long>(u);
}
// COMPILED_PROGRAM
kernel void prepare(device const half*h,device const long*p,device const uint4*m,device half*x,constant uint4&cfg,uint i[[thread_position_in_grid]]){
 if(i>=cfg.x)return;uint4 d=m[i];bool safe=d.z;
 for(uint j=0;j<4;j++)safe=safe&&(p[i*4+j]>=-2048&&p[i*4+j]<=2048);
 for(uint j=0;j<cfg.z;j++)x[i*cfg.z+j]=j<512?h[d.x*512+j]:half(0);
 if(safe){for(uint j=0;j<3;j++)x[i*cfg.z+512+d.y*4+j]=half(p[i*4+j]);x[i*cfg.z+512+d.y*4+3]=1;x[i*cfg.z+512+cfg.w*4]=half(p[i*4+3]);}
}
kernel void fused_mma(device const half*x,device const half*w,device float*y,constant uint4&cfg,uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 uint row=tile*8;threadgroup half left[64],right[64];threadgroup float answer[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<cfg.z;k+=8){
  for(uint q=lane;q<64;q+=32){uint r=q/8,c=q%8;left[q]=row+r<cfg.x?x[(row+r)*cfg.z+k+c]:half(0);right[q]=w[(k+r)*8+c];}
  threadgroup_barrier(mem_flags::mem_threadgroup);simdgroup_half8x8 a,b;simdgroup_load(a,left,8);simdgroup_load(b,right,8);simdgroup_multiply_accumulate(acc,a,b,acc);threadgroup_barrier(mem_flags::mem_threadgroup);
 }
 simdgroup_store(acc,answer,8);threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint q=lane;q<64;q+=32){uint r=row+q/8,c=q%8;if(r<cfg.x&&c<3)y[r*3+c]=answer[q];}
}
kernel void integers(device const long*p,device const uint4*m,device const float*y,device long*out,constant uint4&cfg,uint i[[thread_position_in_grid]]){
 if(i>=cfg.x)return;uint4 d=m[i];long v[16];for(uint j=0;j<3;j++)v[j]=p[i*4+j];bool ok=d.z;long val=0,delta=0,cold=p[i*4+3];
 evaluate(d.y,v,ok,val);uint flags=0;
 if(ok){flags=1|(d.w&&val<cold?2:0)|(!d.w?4:0)|(val<0?8:0);bool dk=true;delta=checked(cold,val,1,dk);if(d.w&&dk)flags|=16;else delta=0;
  bool safe=true;for(uint j=0;j<4;j++)safe=safe&&(p[i*4+j]>=-2048&&p[i*4+j]<=2048);
  if(safe){flags|=32;if(long(y[i*3+1])!=val||(d.w&&long(y[i*3+2])!=delta))flags|=64;
   // Audit mode checks the original integer DAG too. Downstream columns
   // actually consume the GEMM result for these certified-range tuples.
   val=long(y[i*3+1]);delta=d.w?long(y[i*3+2]):0;
   flags=(flags&~10u)|(d.w&&val<cold?2u:0u)|(val<0?8u:0u);
  }
 }else val=0;
 out[i*3]=val;out[i*3+1]=delta;out[i*3+2]=flags;
}
float slog(long v){return copysign(log2(1.0f+abs(float(v)))/64.0f,float(v));}
kernel void pool(device const long*p,device const uint4*m,device const long*out,device const uint*offset,device const uint*index,device float*features,constant uint4&cfg,uint root[[thread_position_in_grid]]){
 if(root>=cfg.y)return;uint start=offset[root],end=offset[root+1];float f[24];for(uint i=0;i<24;i++)f[i]=0;
 float omin=INFINITY,omax=-INFINITY,nmin=INFINITY,nmax=-INFINITY,dmin=INFINITY,dmax=-INFINITY;uint nc=0,no=0,nv=0,nd=0;
 for(uint q=start;q<end;q++){uint i=index[q];uint4 d=m[i];uint flags=uint(out[i*3+2]);
  nc+=d.z;no+=d.w;if(d.w){float z=slog(p[i*4+3]);omin=min(omin,z);omax=max(omax,z);}
  if(d.z){for(uint j=0;j<3;j++)f[5+j]+=slog(p[i*4+j]);}
  if(flags&1){nv++;float z=slog(out[i*3]);nmin=min(nmin,z);nmax=max(nmax,z);f[10]+=z;}
  if(flags&16){nd++;float z=slog(out[i*3+1]);dmin=min(dmin,z);dmax=max(dmax,z);f[13]+=z;}
  if(flags&2){f[16]=1;f[17]++;}if(flags&4){f[18]=1;f[19]++;}
  if((flags&1)&&d.w&&out[i*3]==p[i*4+3]){f[20]=1;f[21]++;}
  if(d.z&&!(flags&1))f[22]++;if((flags&1)&&!(flags&32))f[23]++;
 }
 float count=max(1.0f,float(end-start));f[0]=log2(1.0f+float(end-start))/16.0f;f[1]=float(nc)/count;f[2]=float(no)/count;
 f[3]=no?omin:0;f[4]=no?omax:0;for(uint j=5;j<8;j++)f[j]/=max(1.0f,float(nc));
 f[8]=nv?nmin:0;f[9]=nv?nmax:0;f[10]/=max(1.0f,float(nv));f[11]=nd?dmin:0;f[12]=nd?dmax:0;f[13]/=max(1.0f,float(nd));f[14]=float(nv)/count;f[15]=log2(1.0f+float(nv))/16.0f;
 for(uint j=17;j<24;j++)if(j!=18&&j!=20)f[j]/=count;
 for(uint j=0;j<24;j++)features[root*24+j]=f[j];
}
