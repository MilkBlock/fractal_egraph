#include <metal_stdlib>
#include <metal_simdgroup_matrix>
using namespace metal;
// meta: active, changed, completed Runs, error, newly published facts.
kernel void begin_run(device const uint*state[[buffer(0)]],device uint*next[[buffer(1)]],
 device uint*meta[[buffer(2)]],constant uint&count[[buffer(3)]],uint i[[thread_position_in_grid]]){
 if(!meta[0])return;if(i<count)next[i]=state[i];if(!i)meta[1]=0;
}
// Eight independent continuations occupy one matrix row. All eight result
// columns have consumers; zeros outside their blocks remain measured overhead.
kernel void gather_context(device const uint*state[[buffer(0)]],device const uint*deps[[buffer(1)]],
 device half*x[[buffer(2)]],device const uint*meta[[buffer(3)]],constant uint4&c[[buffer(4)]],uint i[[thread_position_in_grid]]){
 if(!meta[0]||i>=c.x)return;uint row=i/8,block=i%8,at=row*c.z+block*(c.y+1);
 x[at]=half(1);for(uint j=0;j<c.y;j++)x[at+1+j]=half(state[deps[i*c.y+j]]);
}
kernel void closure_gemm(device const half*x[[buffer(0)]],device const half*w[[buffer(1)]],
 device float*scores[[buffer(2)]],device const uint*meta[[buffer(3)]],constant uint4&c[[buffer(4)]],
 uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 if(!meta[0])return;
 threadgroup half left[64],right[64];threadgroup float result[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<c.z;k+=8){
  for(uint z=lane;z<64;z+=32){uint r=z/8,col=z%8,row=tile*8+r;
   left[z]=row<c.w?x[row*c.z+k+col]:half(0);right[z]=w[(k+r)*8+col];
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  simdgroup_half8x8 a,b;simdgroup_load(a,left,8);simdgroup_load(b,right,8);
  simdgroup_multiply_accumulate(acc,a,b,acc);threadgroup_barrier(mem_flags::mem_threadgroup);
 }
 simdgroup_store(acc,result,8);threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint z=lane;z<64;z+=32){uint i=tile*64+z;if(i<c.x)scores[i]=result[z];}
}
kernel void publish(device const float*scores[[buffer(0)]],device const uint*targets[[buffer(1)]],
 device atomic_uint*next[[buffer(2)]],device uint*birth[[buffer(3)]],device uint*meta[[buffer(4)]],
 constant uint&count[[buffer(5)]],constant float&threshold[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(!meta[0]||i>=count||scores[i]<=threshold)return;
 uint target=targets[i];
 if(!atomic_exchange_explicit(next+target,1u,memory_order_relaxed)){
  birth[target]=meta[2]+1;
  atomic_store_explicit((device atomic_uint*)(meta+1),1u,memory_order_relaxed);
  atomic_fetch_add_explicit((device atomic_uint*)(meta+4),1u,memory_order_relaxed);
 }
}
kernel void finish_run(device uint*meta[[buffer(0)]],constant uint&count[[buffer(1)]],uint i[[thread_position_in_grid]]){
 if(i||!meta[0])return;meta[2]++;
 // The finite monotone lattice permits at most count changing rounds plus
 // one quiet round. This is a semantic bound, not a time/search budget.
 if(meta[2]>count+1){meta[3]=1;meta[0]=0;return;}
 if(!meta[1])meta[0]=0;
}
// Same compiled carrier and publication semantics, without GEMM or a model.
// This control separates the value of fixed wiring from matrix arithmetic.
kernel void direct_closure(device const uint*state[[buffer(0)]],device const uint*deps[[buffer(1)]],
 device const uint*targets[[buffer(2)]],device atomic_uint*next[[buffer(3)]],device uint*birth[[buffer(4)]],
 device uint*meta[[buffer(5)]],constant uint4&c[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(!meta[0]||i>=c.x)return;
 for(uint j=0;j<c.y;j++)if(!state[deps[i*c.y+j]])return;
 uint target=targets[i];
 if(!atomic_exchange_explicit(next+target,1u,memory_order_relaxed)){
  birth[target]=meta[2]+1;
  atomic_store_explicit((device atomic_uint*)(meta+1),1u,memory_order_relaxed);
  atomic_fetch_add_explicit((device atomic_uint*)(meta+4),1u,memory_order_relaxed);
 }
}
