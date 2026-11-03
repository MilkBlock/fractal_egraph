// Dense blocks selected from actual shared-coordinate/condition demand.
// Extra cells in a rectangular block are never interpreted as requested facts.
struct SharedJob {uint rows,cols,rowStart,colStart,out,tileEnd,stride;};
kernel void shared_condition_mma(
    device const SharedJob*jobs[[buffer(0)]],device const half*x[[buffer(1)]],
    device const half*w[[buffer(2)]],device const uint*rows[[buffer(3)]],
    device const uint*cols[[buffer(4)]],device const float*bias[[buffer(5)]],
    device float*out[[buffer(6)]],constant uint2&cfg[[buffer(7)]],
    uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]) {
    uint lo=0,hi=cfg.x;
    while(lo<hi){uint m=(lo+hi)/2;if(jobs[m].tileEnd<=tile)lo=m+1;else hi=m;}
    SharedJob j=jobs[lo];uint start=lo?jobs[lo-1].tileEnd:0,local=tile-start;
    uint row=(local/((j.cols+7)/8))*8,col=(local%((j.cols+7)/8))*8;
    threadgroup half left[64],right[64];threadgroup float answer[64];
    simdgroup_float8x8 acc(0.0f);
    for(uint k=0;k<256;k+=8){
        for(uint z=lane;z<64;z+=32){uint r=z/8,c=z%8;
            left[z]=row+r<j.rows?x[rows[j.rowStart+row+r]*256+k+c]:half(0);
            right[z]=col+c<j.cols?w[(k+r)*cfg.y+cols[j.colStart+col+c]]:half(0);
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
        simdgroup_half8x8 a,b;simdgroup_load(a,left,8);simdgroup_load(b,right,8);
        simdgroup_multiply_accumulate(acc,a,b,acc);
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    simdgroup_store(acc,answer,8);threadgroup_barrier(mem_flags::mem_threadgroup);
    for(uint z=lane;z<64;z+=32){uint r=row+z/8,c=col+z%8;
        if(r<j.rows && c<j.cols)out[j.out+r*j.cols+c]=bias[cols[j.colStart+c]]+answer[z]/256.0f;
    }
}
