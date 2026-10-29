// A private Saturate state. Global rows are read-only until ms_publish.
#include <metal_simdgroup_matrix>
// MATRIX_SAT_PROGRAM
// cfg = set, entry rows, equality-ID extent, fact coordinates, marker count,
//       new output count, padded output count, exact-audit flag.
#define MS_ARGS device long*rows[[buffer(0)]],device Meta&m[[buffer(1)]],device uint*wm[[buffer(2)]],device uint*c[[buffer(3)]],constant uint*cfg[[buffer(4)]]
kernel void ms_count(MS_ARGS,device uint*offset[[buffer(5)]],device uint*blocks[[buffer(6)]],uint lane[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 threadgroup uint sum[256];uint n=lane<cfg[1]?ms_visit(cfg[0],lane,cfg[2],rows,offset,0,false):0;sum[tid]=n;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint x=tid>=d?sum[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sum[tid]+=x;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<cfg[1])offset[lane]=sum[tid]-n;if(tid==255)blocks[group]=sum[tid];
}
kernel void ms_count_offsets(MS_ARGS,device uint*blocks[[buffer(5)]],uint lane[[thread_position_in_grid]]){
 if(lane)return;uint n=0;for(uint g=0;g<(cfg[1]+255)/256;g++){uint v=blocks[g];blocks[g]=n;n+=v;}c[0]=n;
}
kernel void ms_init(MS_ARGS,device float*state[[buffer(5)]],device uint*hidden[[buffer(6)]],device uint*birth[[buffer(7)]],device uint*stamp[[buffer(8)]],device uint*fired[[buffer(9)]],uint i[[thread_position_in_grid]]){
 if(i<cfg[3]){state[i]=0;hidden[i]=0;birth[i]=0;stamp[i]=0;}if(i<cfg[4])fired[i]=0;
 if(i==0){for(uint j=1;j<16;j++)c[j]=0;c[8]=m.count;c[9]=m.next_id;c[10]=m.next_fact;c[11]=m.epoch;}
}
kernel void ms_seed(MS_ARGS,device float*state[[buffer(5)]],device uint*hidden[[buffer(6)]],device uint*stamp[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i>=cfg[1])return;uint table=uint(rows[i*STRIDE]);for(uint p=MS_START[cfg[0]];p<MS_START[cfg[0]+1];p++)if(table==MS_TABLES[p]){
 uint f=(p-MS_START[cfg[0]])*cfg[2]+uint(rows[i*STRIDE+4]);hidden[f]=rows[i*STRIDE+1]!=0;state[f]=hidden[f]?0.0f:1.0f;stamp[f]=uint(rows[i*STRIDE+2]);}
}
kernel void ms_pack(MS_ARGS,device const uint*offset[[buffer(5)]],device const uint*blocks[[buffer(6)]],device uint*markers[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(i<cfg[1])ms_visit(cfg[0],i,cfg[2],rows,markers,offset[i]+blocks[i/256],true);
}
kernel void ms_step_begin(MS_ARGS,device const float*state[[buffer(5)]],device float*next[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(c[3])return;if(i<cfg[3])next[i]=state[i];if(i==0){c[5]=0;c[6]=0;}
}
kernel void ms_gather(MS_ARGS,device const float*state[[buffer(5)]],device const uint*markers[[buffer(6)]],device half*x[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(c[3]||i>=cfg[4])return;uint q=i*MS_RECORD;for(uint j=0;j<MS_WIDTH;j++)x[i*MS_WIDTH+j]=j==0?half(1):(j<=markers[q]?half(state[markers[q+5+j]]):half(0));
}
kernel void ms_mma(MS_ARGS,device const half*x[[buffer(5)]],device const half*w[[buffer(6)]],device float*y[[buffer(7)]],uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
 if(c[3])return;uint cols=MS_HEADS/8,row=(tile/cols)*8,col=(tile%cols)*8;threadgroup half a[64],b[64];threadgroup float out[64];simdgroup_float8x8 acc(0.0f);
 for(uint k=0;k<MS_WIDTH;k+=8){for(uint q=lane;q<64;q+=32){uint rr=q/8,cc=q%8;a[q]=row+rr<cfg[4]?x[(row+rr)*MS_WIDTH+k+cc]:half(0);b[q]=w[(k+rr)*MS_HEADS+col+cc];}
 threadgroup_barrier(mem_flags::mem_threadgroup);simdgroup_half8x8 aa,bb;simdgroup_load(aa,a,8);simdgroup_load(bb,b,8);simdgroup_multiply_accumulate(acc,aa,bb,acc);threadgroup_barrier(mem_flags::mem_threadgroup);}
 simdgroup_store(acc,out,8);threadgroup_barrier(mem_flags::mem_threadgroup);for(uint q=lane;q<64;q+=32){uint rr=row+q/8,cc=col+q%8;if(rr<cfg[4])y[rr*MS_HEADS+cc]=out[q];}
}
kernel void ms_apply(MS_ARGS,device const float*state[[buffer(5)]],device float*next[[buffer(6)]],device const uint*markers[[buffer(7)]],device const float*y[[buffer(8)]],device uint*birth[[buffer(9)]],device const uint*hidden[[buffer(10)]],device const uint*stamp[[buffer(11)]],device uint*fired[[buffer(12)]],uint i[[thread_position_in_grid]]){
 if(c[3]||i>=cfg[4])return;uint q=i*MS_RECORD,k=markers[q],head=markers[q+1],rule=markers[q+3];bool semi=markers[q+4];
 bool prediction=y[i*MS_HEADS+head]>MS_THRESHOLD[head],exact=true;
 // Runtime checks all proposed effects. Audit additionally checks negatives.
 if(prediction||cfg[7])for(uint j=0;j<k;j++)exact=exact&&state[markers[q+6+j]]==1.0f;
 if((prediction&&!exact)||(cfg[7]&&prediction!=exact)){atomic_store_explicit((device atomic_uint*)(c+4),1u,memory_order_relaxed);return;}
 if(!prediction||(semi&&fired[i]))return;uint latest=markers[q+5];bool fresh=false;
 for(uint j=0;j<k;j++){uint f=markers[q+6+j];latest=max(latest,stamp[f]);fresh=fresh||birth[f]>0;}
 if(semi&&!fresh&&latest<=wm[rule])return;fired[i]=1;atomic_fetch_add_explicit((device atomic_uint*)(c+6),1u,memory_order_relaxed);
 for(uint j=0;j<markers[q+2];j++){uint f=markers[q+6+MS_K+j];if(hidden[f])continue;
 uint previous=atomic_exchange_explicit((device atomic_uint*)(next+f),as_type<uint>(1.0f),memory_order_relaxed);
 if(!previous){birth[f]=c[1]+1;atomic_fetch_add_explicit((device atomic_uint*)(c+5),1u,memory_order_relaxed);}}
}
kernel void ms_step_finish(MS_ARGS,device uint*history[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(i||c[3])return;history[2*c[1]]=c[5];history[2*c[1]+1]=c[6];c[1]++;c[2]+=c[5];
 if(!c[5]||c[4])c[3]=1;if(c[1]>cfg[3]+1){c[4]=2;c[3]=1;}
}
kernel void ms_output_scan(MS_ARGS,device const uint*birth[[buffer(5)]],device uint*offset[[buffer(6)]],device uint*blocks[[buffer(7)]],uint lane[[thread_position_in_grid]],uint tid[[thread_index_in_threadgroup]],uint group[[threadgroup_position_in_grid]]){
 threadgroup uint sum[256];uint live=lane<cfg[3]&&birth[lane]>0;sum[tid]=live;threadgroup_barrier(mem_flags::mem_threadgroup);
 for(uint d=1;d<256;d*=2){uint v=tid>=d?sum[tid-d]:0;threadgroup_barrier(mem_flags::mem_threadgroup);sum[tid]+=v;threadgroup_barrier(mem_flags::mem_threadgroup);}
 if(lane<cfg[3])offset[lane]=sum[tid]-live;if(tid==255)blocks[group]=sum[tid];
}
kernel void ms_output_offsets(MS_ARGS,device uint*blocks[[buffer(5)]],device uint*order[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i<cfg[6])order[i]=0xffffffff;if(i==0){uint n=0;for(uint j=0;j<(cfg[3]+255)/256;j++){uint v=blocks[j];blocks[j]=n;n+=v;}if(n!=c[2])c[4]=3;}
}
kernel void ms_output_scatter(MS_ARGS,device const uint*birth[[buffer(5)]],device const uint*offset[[buffer(6)]],device const uint*blocks[[buffer(7)]],device uint*order[[buffer(8)]],uint i[[thread_position_in_grid]]){
 if(i<cfg[3]&&birth[i])order[blocks[i/256]+offset[i]]=i;
}
bool ms_before(uint a,uint b,device const uint*birth){if(a==0xffffffff)return false;if(b==0xffffffff)return true;return birth[a]==birth[b]?a<b:birth[a]<birth[b];}
kernel void ms_sort(MS_ARGS,device uint*order[[buffer(5)]],device const uint*birth[[buffer(6)]],constant uint2&stage[[buffer(7)]],uint i[[thread_position_in_grid]]){
 uint j=i^stage.y;if(i>=cfg[6]||j<=i)return;uint a=order[i],b=order[j];if(ms_before(a,b,birth)!=((i&stage.x)==0)){order[i]=b;order[j]=a;}
}
kernel void ms_publish(MS_ARGS,device const uint*order[[buffer(5)]],device const uint*birth[[buffer(6)]],device uint*parent[[buffer(7)]],device uint*fp[[buffer(8)]],uint i[[thread_position_in_grid]]){
 if(c[4]||i>=cfg[5])return;uint f=order[i],pid=f/cfg[2],key=f%cfg[2],t=MS_TABLES[MS_START[cfg[0]]+pid],row=c[8]+i,id=c[9]+i,fact=c[10]+i;
 rows[row*STRIDE]=t;rows[row*STRIDE+1]=0;rows[row*STRIDE+2]=c[11]+birth[f];rows[row*STRIDE+3]=fact;rows[row*STRIDE+4]=key;rows[row*STRIDE+5]=id;rows[CONSTRUCTOR_PRED+row]=id;
 parent[id]=id;parent[MERGE_KEY_FLAGS+id]=MERGE_KEY_OUTPUT[t];fp[fact]=fact;
}
kernel void ms_index(MS_ARGS,device const uint*order[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(c[4]||i>=cfg[5])return;uint row=c[8]+i,t=uint(rows[row*STRIDE]);ulong h=ulong(t)+0x9e3779b97f4a7c15UL,v=ulong(rows[row*STRIDE+4]);v^=v>>30;v*=0xbf58476d1ce4e5b9UL;h^=v+(h<<6)+(h>>2);uint slot=uint(h^(h>>32))&(HASH_CAP-1);
 device atomic_uint*idx=(device atomic_uint*)(rows+INDEX_OFFSET+1);while(true){uint old=atomic_load_explicit(idx+2*slot,memory_order_relaxed);if(!old){uint expected=0;if(atomic_compare_exchange_weak_explicit(idx+2*slot,&expected,row+1,memory_order_relaxed,memory_order_relaxed))return;}else slot=(slot+1)&(HASH_CAP-1);}
}
kernel void ms_commit(MS_ARGS,device const uint*history[[buffer(5)]],uint i[[thread_position_in_grid]]){
 if(i||c[4])return;m.count=c[8]+c[2];m.next_id=c[9]+c[2];m.next_fact=c[10]+c[2];m.epoch=c[11]+c[1];m.rounds+=c[1];rows[ROW_WORDS+1]=m.epoch;m.changed=c[2]>0;m.matched=history[2*(c[1]-1)+1];
 for(uint j=SET_START[cfg[0]];j<SET_START[cfg[0]+1];j++)wm[SET_RULES[j]]=m.epoch-1;
}
