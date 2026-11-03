// Complete per-physical-block summaries, not sampled Cell observations.
// Each set bit represents a possible stored equality ID. Container columns
// conservatively force inspection because a nested member may have changed.
uint ir_bit(device uint*p,uint id){
 if(p[IR_CONTROL]&2u)return 0;
 uint h=id;h^=h>>16;h*=0x7feb352du;h^=h>>15;h*=0x846ca68bu;h^=h>>16;
 return h&1023u;
}
void ir_union(device uint*p,uint displaced){
 if(!(p[IR_CONTROL]&1u))return;
 uint b=ir_bit(p,displaced);
 atomic_fetch_or_explicit((device atomic_uint*)(p+IR_PENDING+b/32),1u<<(b%32),memory_order_relaxed);
}
void ir_dirty_row(device long*r,uint row){
 // New append positions are detected by block length; ordinary updates only
 // change the result column. Scalar-result changes preserve the ID summary.
 uint t=uint(r[row*STRIDE]);
 if(r[IR_BASE]&&EQ[t*COLS+WIDTH[t]-1]!=0)atomic_store_explicit((device atomic_uint*)(r+IR_BASE+IR_HEADER+(row/IR_BLOCK)*IR_STRIDE+1),1u,memory_order_relaxed);
}
void ir_collect(device long*r,device uint*p,uint row,threadgroup atomic_uint*bits,threadgroup atomic_uint*container){
 if(r[row*STRIDE+1]==2)return;
 uint table=uint(r[row*STRIDE]);
 for(uint col=0;col<WIDTH[table];col++){
  uint kind=EQ[table*COLS+col];
  if(kind==1){uint b=ir_bit(p,uint(r[row*STRIDE+4+col]));atomic_fetch_or_explicit(bits+b/32,1u<<(b%32),memory_order_relaxed);}
  else if(kind==2)atomic_store_explicit(container,1u,memory_order_relaxed);
 }
}
// IR_KERNEL_BODY
 if(m.error||stats[10]!=6||control[0]!=1)return;
 // The shared hash scratch still belongs to the unchanged duplicate grouper.
 if(lane<HASH_CAP)qheads[lane]=0;
 uint block=lane/IR_BLOCK,start=block*IR_BLOCK;
 if(start>=control[1])return;
 device long*b=r+IR_BASE+IR_HEADER+block*IR_STRIDE;
 uint count=min(uint(IR_BLOCK),control[1]-start);
 threadgroup uint inspect;
 threadgroup atomic_uint bits[IR_WORDS],container,changed;
 if(tid<IR_WORDS)atomic_store_explicit(bits+tid,0u,memory_order_relaxed);
 if(!tid){
  bool hit=!r[IR_BASE]||!b[0]||b[1]||b[2]||uint(b[3])!=count;
  if(!hit)for(uint w=0;w<IR_WORDS;w++)hit|=(uint(b[9+w])&p[IR_SNAPSHOT+w])!=0;
  inspect=hit;b[4]++;if(!hit)b[5]++;else b[6]+=count;
  atomic_store_explicit(&container,0u,memory_order_relaxed);
  atomic_store_explicit(&changed,0u,memory_order_relaxed);
 }
 threadgroup_barrier(mem_flags::mem_threadgroup);
 if(!inspect)return;
 if(lane<control[1]&&r[lane*STRIDE+1]!=2){
  uint t=uint(r[lane*STRIDE]);bool updated=false;
  for(uint c=0;c<WIDTH[t];c++){
   uint at=lane*STRIDE+4+c,kind=EQ[t*COLS+c];long oldv=r[at];
   long v=kind==1?long(rb_root(p,uint(oldv))):(kind==2?rb_set(r,oldv):oldv);
   if(v!=oldv||(kind==2&&r[ROW_WORDS+v+3]==m.epoch)){
    atomic_store_explicit((device atomic_uint*)(r+RC_VERSION+t),m.epoch,memory_order_relaxed);
    if(c+1<WIDTH[t])atomic_store_explicit((device atomic_uint*)(r+IR_BASE+2),1u,memory_order_relaxed);
    rc_fact_dirty(r,lane);r[at]=v;r[lane*STRIDE+2]=m.epoch;updated=true;
    atomic_store_explicit((device atomic_uint*)&m.changed,1u,memory_order_relaxed);
   }
  }
  if(updated)atomic_fetch_add_explicit(&changed,1u,memory_order_relaxed);
  if(r[IR_BASE])ir_collect(r,p,lane,bits,&container);
 }
 threadgroup_barrier(mem_flags::mem_threadgroup|mem_flags::mem_device);
 if(r[IR_BASE]){
  if(tid<IR_WORDS)b[9+tid]=atomic_load_explicit(bits+tid,memory_order_relaxed);
  if(!tid){b[0]=1;b[1]=0;b[2]=atomic_load_explicit(&container,memory_order_relaxed);b[3]=count;b[8]++;}
 }
 if(!tid)b[7]+=atomic_load_explicit(&changed,memory_order_relaxed);
// IR_KERNEL_BODY
 if(m.error||stats[10]!=6||control[0]!=0)return;
 uint block=lane/IR_BLOCK,start=block*IR_BLOCK;
 // block dispatch may use a scratch buffer larger than the live row range.
 device long*b=r+IR_BASE+IR_HEADER+block*IR_STRIDE;
 if(start>=control[1]){
  // Compaction dispatch covers the OLD row extent. Invalidate evacuated blocks
  // so a later append cannot reuse a full-length summary for different facts.
  if(!tid)b[0]=0;
  if(!lane){m.count=control[1];r[IR_BASE+1]=long(control[1])+1;r[INDEX_OFFSET]=0;r[QUERY_DIRTY]=1;}
  return;
 }
 uint count=min(uint(IR_BLOCK),control[1]-start);
 threadgroup uint refresh;
 threadgroup atomic_uint bits[IR_WORDS],container;
 if(!tid){refresh=r[IR_BASE]&&(control[1]!=control[2]||!b[0]||b[1]||uint(b[3])!=count);atomic_store_explicit(&container,0u,memory_order_relaxed);}
 if(tid<IR_WORDS)atomic_store_explicit(bits+tid,0u,memory_order_relaxed);
 threadgroup_barrier(mem_flags::mem_threadgroup);
 if(lane<control[1]){
  if(control[1]!=control[2])for(uint c=0;c<STRIDE;c++)r[lane*STRIDE+c]=scratch[(instance*batchCfg.z+lane)*STRIDE+c];
  r[RC_FACT_ROW+uint(r[lane*STRIDE+3])]=lane+1;
  if(refresh)ir_collect(r,p,lane,bits,&container);
 }
 threadgroup_barrier(mem_flags::mem_threadgroup|mem_flags::mem_device);
 if(refresh){
  if(tid<IR_WORDS)b[9+tid]=atomic_load_explicit(bits+tid,memory_order_relaxed);
  if(!tid){b[0]=1;b[1]=0;b[2]=atomic_load_explicit(&container,memory_order_relaxed);b[3]=count;b[8]++;}
 }
 if(!lane){m.count=control[1];r[IR_BASE+1]=long(control[1])+1;r[INDEX_OFFSET]=0;r[QUERY_DIRTY]=1;}
