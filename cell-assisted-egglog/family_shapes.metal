// Exact shapes share rows and indexes, not maximum-sized thread workspaces.
kernel void family_shape_dispatch(CTX,uint lane [[thread_position_in_grid]]){
 if(lane)return;device uint* r=(device uint*)routing;uint offset=0;
 r[12]=max(1u,(r[0]+127)/128);r[13]=1;r[14]=1;
 for(uint g=0;g<SHAPE_COUNT;g++){
  uint at=28+g*6;r[at+1]=max(1u,(r[at]+127)/128);r[at+2]=1;r[at+3]=1;r[at+4]=0;r[at+5]=offset;offset+=r[at];
 }
}
kernel void family_shape_scatter(CTX,uint lane [[thread_position_in_grid]]){
 if(atomic_load_explicit(routing+7,memory_order_relaxed)||lane>=atomic_load_explicit(routing,memory_order_relaxed))return;
 uint source=lane*SEED_WORDS,qi=seedQueue[source],q=packet[4+qi*4],at=28+code[QUERY_SHAPE+q]*6;
 uint dest=(atomic_load_explicit(routing+at+5,memory_order_relaxed)+atomic_fetch_add_explicit(routing+at+4,1,memory_order_relaxed))*SEED_WORDS;
 for(uint j=0;j<3;j++)orderedSeeds[dest+j]=seedQueue[source+j];
}
