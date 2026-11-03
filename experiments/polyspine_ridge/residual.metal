#include <metal_stdlib>
using namespace metal;
kernel void residual_copy(device const float*input[[buffer(0)]],device float*output[[buffer(1)]],
 device float*fraction[[buffer(2)]],constant uint&n[[buffer(3)]],uint i[[thread_position_in_grid]]){
 if(i<n){output[i]=input[i];fraction[i]=0;}
}
kernel void residual_step(device const float*input[[buffer(0)]],device atomic_uint*output[[buffer(1)]],
 device atomic_uint*fraction[[buffer(2)]],device const uint*desc[[buffer(3)]],device const uint*rows[[buffer(4)]],
 device const uint*code[[buffer(5)]],constant uint3&c[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i>=c.z)return;
 uint lo=0,hi=c.y;while(lo<hi){uint m=(lo+hi)/2;if(desc[m*9+1]<=i)lo=m+1;else hi=m;}
 uint d=lo*9,base=desc[d+2]+(i-desc[d])*desc[d+3],pc=desc[d+4],aliases=desc[d+5],deps=desc[d+6];
 for(uint j=0;j<aliases;j++){if(rows[base+code[pc]]!=rows[base+code[pc+1]])return;pc+=2;}
 float total=0;for(uint j=0;j<deps;j++){total+=input[code[pc]*c.x+rows[base+code[pc+1]]];pc+=2;}
 uint target=desc[d+7]*c.x+rows[base+desc[d+8]];
 // Preserve AND within each real row, then OR across complete alternatives.
 float truth=float(total==float(deps)),ready=deps?total/float(deps):1.0f;
 atomic_fetch_max_explicit(output+target,as_type<uint>(truth),memory_order_relaxed);
 atomic_fetch_max_explicit(fraction+target,as_type<uint>(ready),memory_order_relaxed);
}
kernel void residual_read(device const float*state[[buffer(0)]],device const float*fraction[[buffer(1)]],
 device const uint*query[[buffer(2)]],device float*features[[buffer(3)]],constant uint4&c[[buffer(4)]],uint i[[thread_position_in_grid]]){
 if(i>=c.x)return;uint q=query[i];
 if(!c.z)features[i*c.y]=state[q];
 else{features[i*c.y+2*c.z-1]=state[q];features[i*c.y+2*c.z]=fraction[q];}
}
