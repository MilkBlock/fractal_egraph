// Earliest legal logical time, not early publication. Positive unary Horn
// clauses only. Each 64-fact block saturates against frozen external ports.
constant uint LOCAL_WIDTH=64;
constant uint NEVER=0xffffffff;
kernel void local_init(device const uint*state[[buffer(0)]],device const uint*hidden[[buffer(1)]],
 device const uint*parent[[buffer(2)]],device uint*distance[[buffer(3)]],device uint*heads[[buffer(4)]],
 constant uint&count[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(i>=count)return;uint p=fact_root(parent,i);distance[i]=state[p]&&!hidden[p]?0:NEVER;heads[i]=NEVER;
}
kernel void local_index(device const uint*rows[[buffer(0)]],device const uint*parent[[buffer(1)]],
 device atomic_uint*heads[[buffer(2)]],device uint*links[[buffer(3)]],device uint*records[[buffer(4)]],
 constant uint*c[[buffer(5)]],uint i[[thread_position_in_grid]]){
 // c: count, arity, flattened base, flattened stride
 if(i>=c[0])return;uint at=i*(c[1]+2),target=fact_root(parent,rows[at+1]),id=c[2]+i;
 uint record=id*c[3],base=(target/LOCAL_WIDTH)*LOCAL_WIDTH;
 records[record]=c[1];records[record+1]=c[4];
 for(uint j=0;j<c[1];j++){
  uint d=fact_root(parent,rows[at+2+j]);bool local=(d-base)<LOCAL_WIDTH;
  records[record+2+j]=local?d-base:LOCAL_WIDTH+d;
 }
 links[id]=atomic_exchange_explicit(heads+target,id,memory_order_relaxed);
}
kernel void local_relax(device const uint*distance[[buffer(0)]],device uint*after[[buffer(1)]],
 device const uint*heads[[buffer(2)]],device const uint*links[[buffer(3)]],device const uint*records[[buffer(4)]],
 device const uint*hidden[[buffer(5)]],device uint*control[[buffer(6)]],
 constant uint4&c[[buffer(7)]],uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 // c: fact count, record stride, schedule period, phase at this epoch's origin
 uint i=tile*LOCAL_WIDTH+lane;threadgroup uint value[64],future[64];threadgroup atomic_uint changed;
 if(!control[0])return;
 value[lane]=i<c.x?distance[i]:NEVER;threadgroup_barrier(mem_flags::mem_threadgroup);
 uint rounds=0;
 while(true){
  if(!lane)atomic_store_explicit(&changed,0u,memory_order_relaxed);
  threadgroup_barrier(mem_flags::mem_threadgroup);
  uint best=value[lane];
  if(i<c.x&&!hidden[i]){
   for(uint edge=heads[i];edge!=NEVER;edge=links[edge]){
    uint r=edge*c.y,k=records[r],phase=records[r+1],ready=0;
    for(uint j=0;j<k;j++){
     uint d=records[r+2+j];ready=max(ready,d<LOCAL_WIDTH?value[d]:distance[d-LOCAL_WIDTH]);
    }
    if(ready==NEVER)continue;
    // The premise must exist strictly BEFORE this ruleset's Run. Rules in
    // the same Run never consume one another's newly published facts.
    ulong shift=(ulong(phase-1)+c.z-((ulong(c.w)+ready)%c.z))%c.z;
    ulong candidate=ulong(ready)+1+shift;
    if(candidate<NEVER)best=min(best,uint(candidate));
   }
  }
  future[lane]=best;
  if(best!=value[lane])atomic_store_explicit(&changed,1u,memory_order_relaxed);
  threadgroup_barrier(mem_flags::mem_threadgroup);
  value[lane]=future[lane];threadgroup_barrier(mem_flags::mem_threadgroup);
  rounds++;
  if(!atomic_load_explicit(&changed,memory_order_relaxed))break;
  // A minimum proof never benefits from a positive-time local cycle.
  if(rounds>LOCAL_WIDTH){if(!lane)atomic_store_explicit((device atomic_uint*)(control+4),1u,memory_order_relaxed);break;}
 }
 if(i<c.x){after[i]=value[lane];if(value[lane]!=distance[i])atomic_store_explicit((device atomic_uint*)(control+1),1u,memory_order_relaxed);}
}
kernel void local_begin_pass(device uint*control[[buffer(0)]],constant uint&unused[[buffer(1)]],uint i[[thread_position_in_grid]]){
 if(!i&&control[0])control[1]=0;
}
kernel void local_end_pass(device uint*control[[buffer(0)]],constant uint&count[[buffer(1)]],uint i[[thread_position_in_grid]]){
 if(i||!control[0])return;control[2]++;
 if(control[2]>count+1||control[4]){control[4]=1;control[0]=0;}
 if(!control[1])control[0]=0;
}
kernel void local_max_time(device const uint*distance[[buffer(0)]],device uint*control[[buffer(1)]],
 constant uint&count[[buffer(2)]],uint i[[thread_position_in_grid]]){
 if(i<count&&distance[i]!=NEVER)atomic_fetch_max_explicit((device atomic_uint*)(control+3),distance[i],memory_order_relaxed);
}
kernel void local_reveal(device uint*state[[buffer(0)]],device const uint*hidden[[buffer(1)]],device const uint*parent[[buffer(2)]],
 device uint*birth[[buffer(3)]],device const uint*distance[[buffer(4)]],device uint*meta[[buffer(5)]],
 constant uint2&c[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i>=c.x||parent[i]!=i||state[i]||hidden[i]||distance[i]>c.y)return;
 state[i]=1;birth[i]=distance[i];
 atomic_fetch_add_explicit((device atomic_uint*)(meta+4),1u,memory_order_relaxed);
 if(distance[i]==c.y)atomic_store_explicit((device atomic_uint*)(meta+1),1u,memory_order_relaxed);
}
