// GPU stores response weights and priors, not per-key dictionaries. Keys below
// are only the current input probes. One grouped matrix kernel handles all banks.
import Foundation
import Metal

func require(_ value:Bool,_ s:String)throws{if !value{throw NSError(domain:s,code:1)}}
try require(CommandLine.arguments.count==4,"worker input.bin reference.bin result.json")
let data=try Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1]))
let magic=String(data:data.prefix(8),encoding:.utf8)
try require(magic=="SKETCH01" || magic=="SPINE001" || magic=="SPINE002" || magic=="SPINE003" || magic=="SPINE004","bad header")
let numericTail=magic=="SPINE004"
let quadratic=magic=="SPINE002"
let dual=magic=="SPINE003" || numericTail
let structural=magic=="SPINE001" || quadratic || dual
var cursor=8
func words(_ n:Int)->[UInt32]{let start=cursor;cursor+=4*n;return data.withUnsafeBytes{r in (0..<n).map{r.loadUnaligned(fromByteOffset:start+4*$0,as:UInt32.self)}}}
func longs(_ n:Int)->[UInt64]{let start=cursor;cursor+=8*n;return data.withUnsafeBytes{r in (0..<n).map{r.loadUnaligned(fromByteOffset:start+8*$0,as:UInt64.self)}}}
func halves(_ n:Int)->[UInt16]{let start=cursor;cursor+=2*n;return data.withUnsafeBytes{r in (0..<n).map{r.loadUnaligned(fromByteOffset:start+2*$0,as:UInt16.self)}}}
let count=Int(words(1)[0])
// Descriptor: rows, full cols, active cols, key start, W start, map start,
// prior start, output start, tile end, output end, padded active cols, width.
var desc:[UInt32]=[],keys:[UInt64]=[],bags:[UInt32]=[],weights:[UInt16]=[],channels:[UInt32]=[],priors:[Float]=[]
var tails:[UInt16]=[]
var totalTiles=0,totalOutput=0,width=0,usefulFlops=0.0,nonzeroFlops=0.0,matrixJobs=0,probeCount=0
for _ in 0..<count{
    let d=words(3).map(Int.init),n=d[0],k=d[1],c=d[2];width=k
    try require(k==((quadratic || dual) ? 512:256),"unexpected coordinate width")
    var rawBags:[[UInt64]]=[],secondBags:[[UInt64]]=[],rawTails:[[UInt16]]=[]
    if structural{for _ in 0..<n{
        rawBags.append(longs(Int(words(1)[0])))
        if dual{secondBags.append(longs(Int(words(1)[0])))}
        if numericTail{rawTails.append(halves(32))}
    }}
    else{rawBags=longs(n).map{[$0]}}
    let w=halves(k*c),prior=words(c).map{Float(bitPattern:$0)}
    let active=(0..<c).filter{j in (0..<k).contains{Float16(bitPattern:w[$0*c+j]) != 0}}
    let keyStart=probeCount,wStart=weights.count,mapStart=channels.count,priorStart=priors.count
    let stride=max(8,(active.count+7)/8*8)
    if !active.isEmpty{
        for (i,first) in rawBags.enumerated(){
            for bag in (dual ? [first,secondBags[i]]:[first]){
                var divisor=1;while divisor*divisor<bag.count{divisor*=2}
                if structural{bags += [UInt32(keys.count),UInt32(bag.count),UInt32(divisor)]}
                keys+=bag
            }
            probeCount+=1
            if numericTail{tails+=rawTails[i]}
        }
        matrixJobs+=1
        for r in 0..<k{for col in 0..<stride{weights.append(col<active.count ? w[r*c+active[col]]:0)}}
        channels+=active.map(UInt32.init)
        totalTiles+=((n+7)/8)*((active.count+7)/8)
        usefulFlops+=Double(2*n*k*active.count)
        nonzeroFlops+=Double(2*n*w.filter{Float16(bitPattern:$0) != 0}.count)
    }
    let outputStart=totalOutput;totalOutput+=n*c;priors+=prior
    desc+=[n,c,active.count,keyStart,wStart,mapStart,priorStart,outputStart,totalTiles,totalOutput,stride,k].map(UInt32.init)
}
try require(cursor==data.count,"trailing packet bytes")
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue()else{fatalError("no Metal GPU")}
func buffer<T>(_ a:[T])->MTLBuffer{
    if a.isEmpty{return device.makeBuffer(length:8,options:.storageModeShared)!}
    return a.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}
}
let description=buffer(desc),keyBuffer=buffer(keys),bagBuffer=buffer(bags),weightBuffer=buffer(weights),channelBuffer=buffer(channels),priorBuffer=buffer(priors)
let tailBuffer=buffer(tails)
let codeBuffer=device.makeBuffer(length:max(8,probeCount*width*2),options:.storageModePrivate)!
let output=device.makeBuffer(length:max(8,totalOutput*4),options:.storageModeShared)!
let source="""
#include <metal_stdlib>
#include <metal_simdgroup_matrix>
using namespace metal;
struct Job {uint rows,cols,active,key,w,map,prior,out,tileEnd,outEnd,stride,width;};
ulong mix64(ulong x){x=(x^(x>>30))*0xbf58476d1ce4e5b9ul;x=(x^(x>>27))*0x94d049bb133111ebul;return x^(x>>31);}
kernel void encode(device const ulong*keys[[buffer(0)]],device half*code[[buffer(1)]],constant uint2&cfg[[buffer(2)]],uint q[[thread_position_in_grid]]){
    if(q>=cfg.x)return;
    for(uint word=0;word<cfg.y/64;word++){
        ulong bits=mix64(keys[q]^(ulong(word+1)*0x9e3779b97f4a7c15ul));
        for(uint bit=0;bit<64;bit++)code[q*cfg.y+word*64+bit]=((bits>>bit)&1ul)?half(1):half(-1);
    }
}
kernel void encode_spine(device const ulong*keys[[buffer(0)]],device const uint*bags[[buffer(1)]],device half*code[[buffer(2)]],constant uint2&cfg[[buffer(3)]],uint q[[thread_position_in_grid]]){
    if(q>=cfg.x)return;
    uint start=bags[3*q],count=bags[3*q+1],divisor=bags[3*q+2];
    for(uint word=0;word<cfg.y/64;word++){
        int sums[64];for(uint bit=0;bit<64;bit++)sums[bit]=0;
        for(uint i=0;i<count;i++){
            ulong bits=mix64(keys[start+i]^(ulong(word+1)*0x9e3779b97f4a7c15ul));
            for(uint bit=0;bit<64;bit++)sums[bit]+=((bits>>bit)&1ul)?1:-1;
        }
        for(uint bit=0;bit<64;bit++)code[q*cfg.y+word*64+bit]=half(float(sums[bit])/float(divisor));
    }
}
kernel void encode_spine_extended(device const ulong*keys[[buffer(0)]],device const uint*bags[[buffer(1)]],device half*code[[buffer(2)]],constant uint4&cfg[[buffer(3)]],uint q[[thread_position_in_grid]]){
    if(q>=cfg.x)return;
    uint linearWidth=cfg.z?cfg.y/2:cfg.y;
    uint planes=cfg.z==2?2:1;
    for(uint plane=0;plane<planes;plane++){
    uint b=(q*planes+plane)*3,start=bags[b],count=bags[b+1],divisor=bags[b+2];
    for(uint word=0;word<linearWidth/64;word++){
        int sums[64];for(uint bit=0;bit<64;bit++)sums[bit]=0;
        for(uint i=0;i<count;i++){
            ulong bits=mix64(keys[start+i]^(ulong(word+1)*0x9e3779b97f4a7c15ul));
            for(uint bit=0;bit<64;bit++)sums[bit]+=((bits>>bit)&1ul)?1:-1;
        }
        for(uint bit=0;bit<64;bit++){
            half value=half(float(sums[bit])/float(divisor));
            code[q*cfg.y+plane*linearWidth+word*64+bit]=value;
            if(cfg.z==1)code[q*cfg.y+linearWidth+word*64+bit]=half(float(value)*float(value));
        }
    }
    }
}
kernel void fill_prior(device const Job*jobs[[buffer(0)]],device const float*prior[[buffer(1)]],device float*out[[buffer(2)]],constant uint2&cfg[[buffer(3)]],uint i[[thread_position_in_grid]]){
    if(i>=cfg.y)return;uint lo=0,hi=cfg.x;
    while(lo<hi){uint m=(lo+hi)/2;if(jobs[m].outEnd<=i)lo=m+1;else hi=m;}
    Job j=jobs[lo];out[i]=prior[j.prior+(i-j.out)%j.cols];
}
kernel void numeric_tail(device half*code[[buffer(0)]],device const half*tail[[buffer(1)]],constant uint2&cfg[[buffer(2)]],uint i[[thread_position_in_grid]]){
    if(i>=cfg.x*32)return;uint row=i/32,col=i%32;code[row*cfg.y+cfg.y-32+col]=tail[i];
}
kernel void grouped_mma(device const Job*jobs[[buffer(0)]],device const half*x[[buffer(1)]],device const half*w[[buffer(2)]],device const uint*map[[buffer(3)]],device const float*prior[[buffer(4)]],device float*out[[buffer(5)]],constant uint&jobCount[[buffer(6)]],uint tile[[threadgroup_position_in_grid]],uint lane[[thread_index_in_threadgroup]]){
    uint lo=0,hi=jobCount;
    while(lo<hi){uint m=(lo+hi)/2;if(jobs[m].tileEnd<=tile)lo=m+1;else hi=m;}
    Job j=jobs[lo];uint start=lo?jobs[lo-1].tileEnd:0,local=tile-start;
    uint colTiles=(j.active+7)/8,row=(local/colTiles)*8,col=(local%colTiles)*8;
    threadgroup half left[64],right[64];threadgroup float answer[64];
    simdgroup_float8x8 acc(0.0f);
    for(uint k=0;k<j.width;k+=8){
        for(uint z=lane;z<64;z+=32){
            uint r=z/8,c=z%8;
            left[z]=row+r<j.rows?x[(j.key+row+r)*j.width+k+c]:half(0);
            right[z]=col+c<j.active?w[j.w+(k+r)*j.stride+col+c]:half(0);
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
        simdgroup_half8x8 a,b;simdgroup_load(a,left,8);simdgroup_load(b,right,8);
        simdgroup_multiply_accumulate(acc,a,b,acc);
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    simdgroup_store(acc,answer,8);threadgroup_barrier(mem_flags::mem_threadgroup);
    for(uint z=lane;z<64;z+=32){uint r=row+z/8,c=col+z%8;if(r<j.rows && c<j.active){uint orig=map[j.map+c];out[j.out+r*j.cols+orig]=prior[j.prior+orig]+answer[z]/float(j.width);}}
}
"""
let options=MTLCompileOptions();options.fastMathEnabled=false
let library=try device.makeLibrary(source:source,options:options)
var pipes:[String:MTLComputePipelineState]=[:]
for name in ["encode","encode_spine","encode_spine_extended","numeric_tail","fill_prior","grouped_mma"]{pipes[name]=try device.makeComputePipelineState(function:library.makeFunction(name:name)!)}
func launch(_ cb:MTLCommandBuffer,_ name:String,_ bs:[MTLBuffer],_ config:[UInt32],_ count:Int,_ groups:Bool=false){
    if count==0{return};let e=cb.makeComputeCommandEncoder()!,p=pipes[name]!;e.setComputePipelineState(p)
    for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
    config.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count)}
    if groups{e.dispatchThreadgroups(MTLSize(width:count,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
    else{e.dispatchThreads(MTLSize(width:count,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,p.maxTotalThreadsPerThreadgroup),height:1,depth:1))}
    e.endEncoding()
}
func execute()throws->[String:Double]{
    let start=ProcessInfo.processInfo.systemUptime,cb=queue.makeCommandBuffer()!
    if quadratic || dual{launch(cb,"encode_spine_extended",[keyBuffer,bagBuffer,codeBuffer],[UInt32(probeCount),UInt32(width),quadratic ? 1:2,0],probeCount)}
    else if structural{launch(cb,"encode_spine",[keyBuffer,bagBuffer,codeBuffer],[UInt32(probeCount),UInt32(width)],probeCount)}
    else{launch(cb,"encode",[keyBuffer,codeBuffer],[UInt32(probeCount),UInt32(width)],probeCount)}
    if numericTail{launch(cb,"numeric_tail",[codeBuffer,tailBuffer],[UInt32(probeCount),UInt32(width)],probeCount*32)}
    launch(cb,"fill_prior",[description,priorBuffer,output],[UInt32(count),UInt32(totalOutput)],totalOutput)
    launch(cb,"grouped_mma",[description,codeBuffer,weightBuffer,channelBuffer,priorBuffer,output],[UInt32(count)],totalTiles,true)
    cb.commit();cb.waitUntilCompleted();try require(cb.status == .completed,"GPU failure \(String(describing:cb.error))")
    return ["gpu_seconds":cb.gpuEndTime-cb.gpuStartTime,"host_submit_wait_seconds":ProcessInfo.processInfo.systemUptime-start]
}
_ = try execute();let timing=try execute()
let reference=try Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[2]))
try require(reference.count==totalOutput*4,"reference size mismatch")
let actual=output.contents().assumingMemoryBound(to:Float.self)
var maxError:Float=0,decisions=0
reference.withUnsafeBytes{ref in for i in 0..<totalOutput{let expected=ref.loadUnaligned(fromByteOffset:i*4,as:Float.self);maxError=max(maxError,abs(actual[i]-expected));if (actual[i]>=0.5) != (expected>=0.5){decisions+=1}}}
try require(maxError<0.01,"GPU coefficient evaluation differs from CPU reference")
let report:[String:Any]=["scope":"standalone speculative response evaluation; no exact GPU key index or native runtime speedup",
    "coordinate_input":structural ? "Analyze-derived constructor spine token bags; no opaque ID feature":"opaque key fingerprints",
    "coordinate_nonlinearity":quadratic ? "append elementwise square":"none",
    "coordinate_blocks":numericTail ? "two structural blocks, last 32 coordinates are explicit numeric context":dual ? "separate local and recursive context":"single structural bag",
    "matrix_row_kind":structural ? "distinct structural signatures":"current probe keys",
    "device":device.name,"jobs":count,"matrix_jobs":matrixJobs,"matrix_tiles":totalTiles,"kernels":numericTail ? 4:3,
    "timing":timing,"outputs":totalOutput,"matrix_probe_keys":probeCount,"coordinate_tokens":keys.count,
    "max_score_error":maxError,"threshold_disagreements":decisions,
    "useful_matrix_flops":usefulFlops,
    "nonzero_coefficient_flops":nonzeroFlops,"scheduled_tile_flops":Double(totalTiles*2*8*8*width),
    "resident_weight_prior_and_routing_bytes":weightBuffer.length+priorBuffer.length+description.length+channelBuffer.length,
    "temporary_code_bytes":codeBuffer.length,"current_probe_bytes":keyBuffer.length+(structural ? bagBuffer.length:0)+(numericTail ? tailBuffer.length:0),"output_bytes":output.length,
    "reference_limit":structural ? "FP16 arithmetic checked against CPU scores; predicate predictions remain speculative; Apple matrix operations are not NVIDIA Tensor Core evidence":"FP16 storage and known-key calibration checked in Python; Apple simdgroup matrix operations are not NVIDIA Tensor Core evidence",
    "warmups":1,"retained_runs":1]
let text=try JSONSerialization.data(withJSONObject:report,options:[.prettyPrinted,.sortedKeys])
try text.write(to:URL(fileURLWithPath:CommandLine.arguments[3]));print(String(data:text,encoding:.utf8)!)
