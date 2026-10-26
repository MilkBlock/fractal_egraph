// Dynamic closure metadata extends the static layout with epoch, changing
// rounds, and rejected stale receipts at positions 5, 6 and 7.
uint fact_root(device const uint*p,uint i){while(p[i]!=i)i=p[i];return i;}
kernel void dyn_initialize(device uint*state[[buffer(0)]],device uint*hidden[[buffer(1)]],
 device uint*parent[[buffer(2)]],device uint*birth[[buffer(3)]],constant uint2&range[[buffer(4)]],uint i[[thread_position_in_grid]]){
 i+=range.x;if(i>=range.y)return;state[i]=0;hidden[i]=0;parent[i]=i;birth[i]=0xffffffff;
}
kernel void dyn_alias(device uint*state[[buffer(0)]],device uint*hidden[[buffer(1)]],device uint*parent[[buffer(2)]],
 device const uint2*pairs[[buffer(3)]],constant uint&count[[buffer(4)]],uint i[[thread_position_in_grid]]){
 if(i)return;
 for(uint j=0;j<count;j++){
  uint a=fact_root(parent,pairs[j].x),b=fact_root(parent,pairs[j].y);if(a==b)continue;
  uint lo=min(a,b),hi=max(a,b);state[lo]|=state[hi];hidden[lo]|=hidden[hi];parent[hi]=lo;
 }
}
kernel void dyn_seed(device atomic_uint*state[[buffer(0)]],device atomic_uint*hidden[[buffer(1)]],
 device const uint*parent[[buffer(2)]],device const uint*seeds[[buffer(3)]],constant uint&count[[buffer(4)]],uint i[[thread_position_in_grid]]){
 if(i>=count)return;uint slot=fact_root(parent,seeds[i*3]);
 atomic_fetch_or_explicit(state+slot,seeds[i*3+1],memory_order_relaxed);
 atomic_fetch_or_explicit(hidden+slot,seeds[i*3+2],memory_order_relaxed);
}
kernel void dyn_reset_birth(device const uint*state[[buffer(0)]],device const uint*parent[[buffer(1)]],
 device uint*birth[[buffer(2)]],constant uint&count[[buffer(3)]],uint i[[thread_position_in_grid]]){
 if(i<count)birth[i]=state[fact_root(parent,i)]?0:0xffffffff;
}
kernel void dyn_rows(device const uint*updates[[buffer(0)]],device uint*rows[[buffer(1)]],
 constant uint2&c[[buffer(2)]],uint i[[thread_position_in_grid]]){
 if(i>=c.x)return;uint start=i*(c.y+1),slot=updates[start];
 for(uint j=0;j<c.y;j++)rows[slot*c.y+j]=updates[start+1+j];
}
kernel void dyn_gather(device const uint*state[[buffer(0)]],device const uint*hidden[[buffer(1)]],device const uint*parent[[buffer(2)]],
 device const uint*rows[[buffer(3)]],device half*x[[buffer(4)]],device uint*receipts[[buffer(5)]],
 device const uint*meta[[buffer(6)]],constant uint4&c[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(!meta[0]||i>=c.x)return;
 uint at=i*(c.y+1),record=i*(c.y+2);x[at]=half(1);
 for(uint j=0;j<c.y;j++){uint slot=fact_root(parent,rows[record+2+j]);x[at+1+j]=half(state[slot]&&!hidden[slot]);}
 receipts[i*3]=meta[5];receipts[i*3+1]=rows[record];receipts[i*3+2]=meta[2];
}
kernel void dyn_publish(device const float*scores[[buffer(0)]],device const uint*rows[[buffer(1)]],
 device const uint*receipts[[buffer(2)]],device const uint*parent[[buffer(3)]],device const uint*hidden[[buffer(4)]],
 device atomic_uint*next[[buffer(5)]],device uint*birth[[buffer(6)]],device uint*meta[[buffer(7)]],
 constant uint4&c[[buffer(8)]],constant float&threshold[[buffer(9)]],uint i[[thread_position_in_grid]]){
 if(!meta[0]||i>=c.x)return;
 if(receipts[i*3]!=meta[5]||!rows[i*(c.y+2)]||receipts[i*3+1]!=rows[i*(c.y+2)]||receipts[i*3+2]!=meta[2]){
  atomic_fetch_add_explicit((device atomic_uint*)(meta+7),1u,memory_order_relaxed);return;
 }
 if(scores[i]<=threshold)return;uint target=fact_root(parent,rows[i*(c.y+2)+1]);
 if(hidden[target])return;
 if(!atomic_exchange_explicit(next+target,1u,memory_order_relaxed)){
  birth[target]=meta[2]+1;atomic_store_explicit((device atomic_uint*)(meta+1),1u,memory_order_relaxed);
  atomic_fetch_add_explicit((device atomic_uint*)(meta+4),1u,memory_order_relaxed);
 }
}
kernel void dyn_direct(device const uint*state[[buffer(0)]],device const uint*hidden[[buffer(1)]],device const uint*parent[[buffer(2)]],
 device const uint*rows[[buffer(3)]],device atomic_uint*next[[buffer(4)]],device uint*birth[[buffer(5)]],
 device uint*meta[[buffer(6)]],constant uint4&c[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(!meta[0]||i>=c.x||!rows[i*(c.y+2)])return;
 for(uint j=0;j<c.y;j++){uint slot=fact_root(parent,rows[i*(c.y+2)+2+j]);if(!state[slot]||hidden[slot])return;}
 uint target=fact_root(parent,rows[i*(c.y+2)+1]);if(hidden[target])return;
 if(!atomic_exchange_explicit(next+target,1u,memory_order_relaxed)){
  birth[target]=meta[2]+1;atomic_store_explicit((device atomic_uint*)(meta+1),1u,memory_order_relaxed);
  atomic_fetch_add_explicit((device atomic_uint*)(meta+4),1u,memory_order_relaxed);
 }
}
kernel void dyn_finish(device uint*meta[[buffer(0)]],constant uint&count[[buffer(1)]],uint i[[thread_position_in_grid]]){
 if(i||!meta[0])return;meta[2]++;
 if(meta[1])meta[6]++;
 if(meta[6]>count){meta[3]=1;meta[0]=0;return;}
 if(!meta[1])meta[0]=0;
}
