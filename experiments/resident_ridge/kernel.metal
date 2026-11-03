#include <metal_stdlib>
#include <metal_simdgroup_matrix>
using namespace metal;
// Compile-time sizes describe one certified frame family, not global egraph IDs.
struct Edge {uint source,target,paramStart,paramCount,partStart,mapStart;};
struct Feature {uint kind,a,b,c;};
struct Config {uint rows,slots,params,states,edges,features,freshBase,freshStride,logSize;};
bool feature_on(Feature f,uint state,uint ready,device const uint*p) {
    if(f.kind==0)return state==f.a;
    if(f.kind==1)return (ready&(1u<<f.a))!=0;
    if(f.kind==2)return (ready&(1u<<f.a)) && (ready&(1u<<f.b)) && p[f.a]==p[f.b];
    if(f.kind==3)return state==f.a && (ready&f.b)==f.b;
    return state==f.a && (ready&(1u<<f.b)) && (ready&(1u<<f.c)) && p[f.b]==p[f.c];
}
kernel void initialize(device const uint*input[[buffer(0)]],device uint*meta[[buffer(1)]],device uint*params[[buffer(2)]],
 device int*sum[[buffer(3)]],device half*x[[buffer(4)]],device const Feature*features[[buffer(5)]],device const int*codes[[buffer(6)]],constant Config&c[[buffer(7)]],uint row[[thread_position_in_grid]]) {
    if(row>=c.rows)return;
    uint ready=input[row*(c.params+1)+c.params];
    for(uint p=0;p<c.params;p++)params[row*c.params+p]=input[row*(c.params+1)+p];
    meta[row*6]=0;meta[row*6+1]=0;meta[row*6+2]=ready;meta[row*6+3]=0;meta[row*6+4]=0xffffffff;
    uint count=0;for(uint f=0;f<c.features;f++)count+=feature_on(features[f],0,ready,params+row*c.params);
    meta[row*6+5]=count;uint divisor=1;while(divisor*divisor<count)divisor*=2;
    for(uint k=0;k<256;k++) {int s=0;for(uint f=0;f<c.features;f++)if(feature_on(features[f],0,ready,params+row*c.params))s+=codes[f*256+k];sum[row*256+k]=s;x[row*256+k]=half(float(s)/float(divisor));}
}
kernel void add_ready(device const uint*patch[[buffer(0)]],device uint*meta[[buffer(1)]],device const uint*params[[buffer(2)]],
 device int*sum[[buffer(3)]],device half*x[[buffer(4)]],device const Feature*features[[buffer(5)]],device const int*codes[[buffer(6)]],constant Config&c[[buffer(7)]],constant uint&patchCount[[buffer(8)]],uint id[[thread_position_in_grid]]) {
    if(id>=patchCount)return;uint row=patch[2*id],mask=patch[2*id+1];if(row>=c.rows)return;
    uint state=meta[row*6],old=meta[row*6+2],ready=old|mask,count=meta[row*6+5];
    for(uint f=0;f<c.features;f++)if(!feature_on(features[f],state,old,params+row*c.params)&&feature_on(features[f],state,ready,params+row*c.params)) {
        count++;for(uint k=0;k<256;k++)sum[row*256+k]+=codes[f*256+k];
    }
    meta[row*6+2]=ready;meta[row*6+5]=count;uint divisor=1;while(divisor*divisor<count)divisor*=2;
    for(uint k=0;k<256;k++)x[row*256+k]=half(float(sum[row*256+k])/float(divisor));
}
kernel void predict(device const half*x[[buffer(0)]],device const half*w[[buffer(1)]],device const float*bias[[buffer(2)]],
 device float*out[[buffer(3)]],constant Config&c[[buffer(4)]],uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]) {
    uint stride=((c.edges+7)/8)*8,rt=tile/((c.edges+7)/8)*8,ct=tile%((c.edges+7)/8)*8;
    threadgroup half left[64],right[64];threadgroup float answer[64];simdgroup_float8x8 acc(0.0f);
    for(uint k=0;k<256;k+=8) {
        for(uint z=lane;z<64;z+=32){uint r=z/8,col=z%8;left[z]=rt+r<c.rows?x[(rt+r)*256+k+col]:half(0);right[z]=w[(k+r)*stride+ct+col];}
        threadgroup_barrier(mem_flags::mem_threadgroup);simdgroup_half8x8 a,b;simdgroup_load(a,left,8);simdgroup_load(b,right,8);
        simdgroup_multiply_accumulate(acc,a,b,acc);threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    simdgroup_store(acc,answer,8);threadgroup_barrier(mem_flags::mem_threadgroup);
    for(uint z=lane;z<64;z+=32){uint r=rt+z/8,col=ct+z%8;if(r<c.rows&&col<c.edges)out[r*c.edges+col]=bias[col]+answer[z]/256.0f;}
}
kernel void advance(device uint*meta[[buffer(0)]],device uint*frame[[buffer(1)]],device const uint*params[[buffer(2)]],device const float*scores[[buffer(3)]],
 device const Edge*edges[[buffer(4)]],device const uint*edgeParams[[buffer(5)]],device const uint*partition[[buffer(6)]],device const int*mapping[[buffer(7)]],
 device const uint*sizes[[buffer(8)]],device const uint*offsets[[buffer(9)]],device const int*next[[buffer(10)]],device uint*events[[buffer(11)]],
 device uint*stats[[buffer(12)]],constant Config&c[[buffer(13)]],uint row[[thread_position_in_grid]]) {
    if(row>=c.rows)return;uint m=row*6,st=row*8;meta[m+4]=0xffffffff;
    if(meta[m+1]>=1)return;uint source=meta[m],ready=meta[m+2],chosen=0xffffffff;bool recovered=false;
    // The source frame was established by a certified prior transfer. Its
    // internal distinctness and wiring are invariants, not rescanned guards.
    // This ABI admits no external writes/unions into existing frame identities.
    bool aliasFailure=false;
    for(uint pass=0;pass<2&&chosen==0xffffffff;pass++)for(uint t=0;t<c.edges;t++) {
        bool proposed=scores[row*c.edges+t]>=0.5f;if(proposed!=(pass==0))continue;
        Edge e=edges[t];bool ok=e.source==source;
        for(uint p=0;p<e.paramCount;p++)ok=ok&&((ready&(1u<<edgeParams[e.paramStart+p]))!=0);
        if(ok){uint n=sizes[source]+e.paramCount;bool alias=true;
            for(uint i=sizes[source];i<n;i++)for(uint j=0;j<i;j++) {
                uint a=i<sizes[source]?frame[row*c.slots+i]:params[row*c.params+edgeParams[e.paramStart+i-sizes[source]]];
                uint b=j<sizes[source]?frame[row*c.slots+j]:params[row*c.params+edgeParams[e.paramStart+j-sizes[source]]];
                alias=alias&&((a==b)==(partition[e.partStart+i]==partition[e.partStart+j]));
            }
            aliasFailure=aliasFailure||!alias;ok=alias;
        }
        stats[st+1]++;if(proposed&&!ok)stats[st+2]++;
        if(ok){chosen=t;recovered=!proposed;break;}
    }
    stats[st]++;
    if(chosen==0xffffffff){meta[m+1]=aliasFailure?2:(next[source]<0?1:0);stats[st+4]+=aliasFailure;stats[st+5]++;return;}
    Edge e=edges[chosen];uint count=meta[m+3];if(count>=c.logSize){meta[m+1]=4;return;}
    uint old[SLOTS];for(uint s=0;s<c.slots;s++)old[s]=frame[row*c.slots+s];
    for(uint s=0;s<c.slots;s++) {
        uint value=0;if(s<sizes[e.target]){int from=mapping[e.mapStart+s];
            if(from<0)value=c.freshBase+row*c.freshStride+offsets[e.target]+s;
            else if(uint(from)<sizes[source])value=old[from];
            else value=params[row*c.params+edgeParams[e.paramStart+uint(from)-sizes[source]]];
        }frame[row*c.slots+s]=value;
    }
    events[row*c.logSize+count]=chosen;meta[m+3]++;meta[m+4]=source;meta[m]=e.target;meta[m+1]=next[e.target]<0?1:0;
    stats[st+3]+=recovered;stats[st+6]++;stats[st+7]+=sizes[e.target];
}
kernel void update_coordinates(device uint*meta[[buffer(0)]],device int*sum[[buffer(1)]],device half*x[[buffer(2)]],
 device const int*codes[[buffer(3)]],device const Feature*features[[buffer(4)]],device const uint*params[[buffer(5)]],constant Config&c[[buffer(6)]],uint row[[thread_position_in_grid]]) {
    if(row>=c.rows)return;uint old=meta[row*6+4];if(old==0xffffffff)return;
    uint now=meta[row*6],ready=meta[row*6+2];int count=int(meta[row*6+5]);
    for(uint f=0;f<c.features;f++){
        int delta=int(feature_on(features[f],now,ready,params+row*c.params))-int(feature_on(features[f],old,ready,params+row*c.params));
        if(delta){count+=delta;for(uint k=0;k<256;k++)sum[row*256+k]+=delta*codes[f*256+k];}
    }
    meta[row*6+5]=uint(count);uint divisor=1;while(divisor*divisor<uint(count))divisor*=2;
    for(uint k=0;k<256;k++)x[row*256+k]=half(float(sum[row*256+k])/float(divisor));
}
kernel void readback(device const uint*meta[[buffer(0)]],device const uint*frame[[buffer(1)]],device const uint*params[[buffer(2)]],
 device const uint*events[[buffer(3)]],device const uint*stats[[buffer(4)]],device uint*out[[buffer(5)]],constant Config&c[[buffer(6)]],uint row[[thread_position_in_grid]]) {
    if(row>=c.rows)return;uint stride=4+c.params+c.slots+c.logSize+8,base=row*stride;
    for(uint i=0;i<4;i++)out[base+i]=meta[row*6+i];base+=4;
    for(uint i=0;i<c.params;i++)out[base+i]=params[row*c.params+i];base+=c.params;
    for(uint i=0;i<c.slots;i++)out[base+i]=frame[row*c.slots+i];base+=c.slots;
    for(uint i=0;i<c.logSize;i++)out[base+i]=events[row*c.logSize+i];base+=c.logSize;
    for(uint i=0;i<8;i++)out[base+i]=stats[row*8+i];
}
kernel void audit_scores(device const half*x[[buffer(0)]],device const float*scores[[buffer(1)]],device float*out[[buffer(2)]],constant Config&c[[buffer(3)]],uint gid[[thread_position_in_grid]]) {
    uint width=256+c.edges;if(gid>=c.rows*width)return;uint row=gid/width,col=gid%width;
    out[gid]=col<256?float(x[row*256+col]):scores[row*c.edges+col-256];
}
