// Persistent token coordinates; a patch changes only named features. Prediction
// rows carry native typed identities outside the learned numeric representation.
kernel void resident_patch(device half*z[[buffer(0)]],device uint*meta[[buffer(1)]],
 device const uint*patches[[buffer(2)]],device const uint2*edits[[buffer(3)]],
 device atomic_uint*error[[buffer(4)]],constant uint2&cfg[[buffer(5)]],uint i[[thread_position_in_grid]]) {
 if(i>=cfg.x)return;uint at=i*5,row=patches[at],version=patches[at+1],sort=patches[at+2];
 if(meta[row*3]+1!=version || (meta[row*3] && meta[row*3+1]!=sort)){
  atomic_fetch_or_explicit(error,1,memory_order_relaxed);return;
 }
 for(uint j=0;j<patches[at+4];j++){uint2 e=edits[patches[at+3]+j];z[row*cfg.y+e.x]=half(e.y);}
 meta[row*3]=version;meta[row*3+1]=sort;
}
// Columns are high coefficient, low coefficient, and exact mask count for each
// head. The left token tile is shared across all these response columns.
kernel void resident_response_mma(device const SharedJob*jobs[[buffer(0)]],device const half*z[[buffer(1)]],
 device const half*w[[buffer(2)]],device const uint*rows[[buffer(3)]],device const uint*cols[[buffer(4)]],
 device float*raw[[buffer(5)]],constant uint4&cfg[[buffer(6)]],
 uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]) {
 uint lo=0,hi=cfg.x;while(lo<hi){uint m=(lo+hi)/2;if(jobs[m].tileEnd<=tile)lo=m+1;else hi=m;}
 SharedJob j=jobs[lo];uint start=lo?jobs[lo-1].tileEnd:0,local=tile-start;
 uint row=(local/((j.cols+7)/8))*8,col=(local%((j.cols+7)/8))*8;
 threadgroup half left[64],right[64];threadgroup float answer[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<cfg.y;k+=8){
  for(uint v=lane;v<64;v+=32){uint r=v/8,c=v%8;
   left[v]=row+r<j.rows && k+c<cfg.y?z[rows[j.rowStart+row+r]*cfg.y+k+c]:half(0);
   right[v]=col+c<j.cols && k+r<cfg.y?w[(k+r)*cfg.z+cols[j.colStart+col+c]]:half(0);
  }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  simdgroup_half8x8 a,b;simdgroup_load(a,left,8);simdgroup_load(b,right,8);simdgroup_multiply_accumulate(acc,a,b,acc);
  threadgroup_barrier(mem_flags::mem_threadgroup);
 }
 simdgroup_store(acc,answer,8);threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint v=lane;v<64;v+=32){uint r=row+v/8,c=col+v%8;
  if(r<j.rows && c<j.cols)raw[rows[j.rowStart+r]*cfg.z+cols[j.colStart+c]]=answer[v];
 }
}
// Score validity is per head. Adding a new consumer need not refresh unrelated
// heads; feature updates invalidate old versions without clearing the matrix.
kernel void resident_normalize(device const uint2*pairs[[buffer(0)]],device const uint*meta[[buffer(1)]],
 device const uint*modelSort[[buffer(2)]],device const uint*constantModel[[buffer(3)]],
 device const float*raw[[buffer(4)]],device const float*bias[[buffer(5)]],device float*scores[[buffer(6)]],
 device uint*scoreVersions[[buffer(7)]],device atomic_uint*error[[buffer(8)]],constant uint2&cfg[[buffer(9)]],uint i[[thread_position_in_grid]]) {
 if(i>=cfg.x)return;uint row=pairs[i].x,m=pairs[i].y,slot=row*cfg.y+m;
 if(modelSort[m]!=meta[row*3+1]){atomic_fetch_or_explicit(error,2,memory_order_relaxed);return;}
 float value=bias[m];
 if(!constantModel[m]){
  uint at=slot*3;float count=raw[at+2];
  if(count<1 || count!=floor(count)){atomic_fetch_or_explicit(error,2,memory_order_relaxed);return;}
  uint d=1;while(d*d<uint(count))d*=2;
  value+=(raw[at]+raw[at+1])/float(d);
 }
 scores[slot]=value;scoreVersions[slot]=meta[row*3];
}
kernel void resident_gather(device const uint*req[[buffer(0)]],device const uint*meta[[buffer(1)]],
 device const uint*modelSort[[buffer(2)]],device const float*scores[[buffer(3)]],device const uint*scoreVersions[[buffer(4)]],device float*out[[buffer(5)]],
 device atomic_uint*error[[buffer(6)]],constant uint2&cfg[[buffer(7)]],uint i[[thread_position_in_grid]]) {
 if(i>=cfg.x)return;uint row=req[i*3],m=req[i*3+1],version=req[i*3+2];
 if(meta[row*3]!=version || scoreVersions[row*cfg.y+m]!=version || meta[row*3+1]!=modelSort[m]){
  atomic_fetch_or_explicit(error,4,memory_order_relaxed);return;
 }
 out[i]=scores[row*cfg.y+m];
}
