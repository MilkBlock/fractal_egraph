// Dense finite-field lookup versus an exact same-filter GPU hash/bitmap path.
// FP16 digit matrices, FP32 accumulation, then exact integer reconstruction.
import Foundation
import Metal
import MetalPerformanceShaders

let prime=65521
struct Report:Encodable {
    let device:String;let layout:String;let batch:Int;let pages:Int;let coefficients:Int;let queries:Int
    let polynomial_gpu_seconds:Double;let hash_gpu_seconds:Double
    let polynomial_host_submit_wait_seconds:Double;let hash_host_submit_wait_seconds:Double
    let prepare_seconds:Double;let exact_filter_and_handle_agreement:Bool
    let candidate_pages:Int;let polynomial_float_operations:Double
    let reference_gemm_seconds:Double;let reference_gemm_float_operations:Double
    let scope:String
}
func require(_ b:Bool,_ m:String)throws{if !b{throw NSError(domain:m,code:1)}}
let input=try Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1]))
let stacked=CommandLine.arguments.contains("--stacked")
let batchArg=CommandLine.arguments.firstIndex(of:"--batch")
let batch=batchArg.map{Int(CommandLine.arguments[$0+1])!} ?? 2048
try require(batch>0 && batch%32==0,"batch must be a positive multiple of 32")
try require(String(data:input.prefix(8),encoding:.utf8)=="POLYKEY1","bad header")
var cursor=8
func word()->Int{let v=input.withUnsafeBytes{$0.loadUnaligned(fromByteOffset:cursor,as:UInt32.self)};cursor+=4;return Int(v)}
let m=word(),k=word(),p=word(),capacity=word(),bitmapWords=word(),modulus=word()
try require(modulus==prime && k>0 && k<=512 && k%32==0 && m>0 && p>0,"unsupported field/layout")
func words(_ n:Int)->[UInt32]{let start=cursor;cursor+=4*n;return input.withUnsafeBytes{raw in (0..<n).map{raw.loadUnaligned(fromByteOffset:start+4*$0,as:UInt32.self)}}}
let coefficients=words(3*m*k),queries=words(2*p),hashTables=words(m*capacity),bitmaps=words(m*bitmapWords)
try require(cursor==input.count,"bad packet length")
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue()else{fatalError("Metal unavailable")}
let source="""
#include <metal_stdlib>
using namespace metal;
constant uint P=65521;
kernel void powers(device const uint2 *query [[buffer(0)]],device half *lo [[buffer(1)]],
                   device half *hi [[buffer(2)]],constant uint4 &cfg [[buffer(3)]],
                   constant uint &which [[buffer(4)]],uint q [[thread_position_in_grid]]){
    uint start=cfg.x,count=cfg.y,width=cfg.z,stride=cfg.w;if(q>=stride)return;
    bool live=q<count;uint value=live?(which==0?query[start+q].x:query[start+q].y):0;
    uint power=1;
    for(uint d=0;d<width;d++){
        int centered=power>P/2?int(power)-int(P):int(power);
        int low=((centered+128)&255)-128,high=(centered-low)/256;
        lo[d*stride+q]=live?half(low):half(0);hi[d*stride+q]=live?half(high):half(0);
        power=(power*value)%P;
    }
}
kernel void powers_packed(device const uint2 *query [[buffer(0)]],device half *out [[buffer(1)]],
                          constant uint4 &cfg [[buffer(2)]],constant uint &which [[buffer(3)]],
                          uint q [[thread_position_in_grid]]){
    uint start=cfg.x,count=cfg.y,width=cfg.z,stride=cfg.w;if(q>=stride)return;
    bool live=q<count;uint value=live?(which==0?query[start+q].x:query[start+q].y):0,power=1;
    for(uint d=0;d<width;d++){
        int centered=power>P/2?int(power)-int(P):int(power);
        int low=((centered+128)&255)-128,high=(centered-low)/256;
        out[d*2*stride+q]=live?half(low):half(0);
        out[d*2*stride+stride+q]=live?half(high):half(0);
        power=(power*value)%P;
    }
}
uint residue(float x){int s=int(x)%int(P);return uint(s<0?s+int(P):s);}
kernel void reconstruct(device const float *a [[buffer(0)]],device const float *b [[buffer(1)]],
                        device const float *c [[buffer(2)]],device const float *d [[buffer(3)]],
                        device uint *out [[buffer(4)]],constant uint &size [[buffer(5)]],uint i [[thread_position_in_grid]]){
    if(i>=size)return;
    out[i]=(residue(a[i])+256*(residue(b[i])+residue(c[i]))+15*residue(d[i]))%P;
}
kernel void reconstruct_packed(device const float *product [[buffer(0)]],device uint *out [[buffer(1)]],
                               constant uint2 &cfg [[buffer(2)]],uint i [[thread_position_in_grid]]){
    uint rows=cfg.x,cols=cfg.y;if(i>=rows*cols)return;
    uint r=i/cols,q=i%cols,top=r*2*cols+q,bottom=(r+rows)*2*cols+q;
    out[i]=(residue(product[top])+256*(residue(product[top+cols])+residue(product[bottom]))
            +15*residue(product[bottom+cols]))%P;
}
kernel void decide(device const uint *first [[buffer(0)]],device const uint *second [[buffer(1)]],
                   device ushort *out [[buffer(2)]],device atomic_uint *error [[buffer(3)]],
                   constant uint4 &cfg [[buffer(4)]],constant uint &total [[buffer(5)]],uint i [[thread_position_in_grid]]){
    uint pages=cfg.x,stride=cfg.y,count=cfg.z,start=cfg.w;if(i>=pages*stride)return;
    uint page=i/stride,q=i%stride;if(q>=count)return;
    uint handle=first[(pages+page)*stride+q];
    bool pass=first[i]==0 && second[i]==0;
    if(pass && handle>=65520){atomic_store_explicit(error,1,memory_order_relaxed);}
    out[page*total+start+q]=pass?ushort(handle+1):ushort(0);
}
kernel void hash_filter(device const uint2 *query [[buffer(0)]],device const uint *table [[buffer(1)]],
                        device const uint *bitmap [[buffer(2)]],device ushort *out [[buffer(3)]],
                        constant uint4 &cfg [[buffer(4)]],uint i [[thread_position_in_grid]]){
    uint pages=cfg.x,queries=cfg.y,capacity=cfg.z,words=cfg.w;if(i>=pages*queries)return;
    uint page=i/queries,q=i%queries,x=query[q].x,y=query[q].y;
    uint slot=(x*2654435761u)&(capacity-1),entry=0;
    for(uint n=0;n<capacity;n++){
        entry=table[page*capacity+slot];if(entry==0 || (entry&65535)==x+1)break;
        slot=(slot+1)&(capacity-1);
    }
    bool pass=entry!=0 && (entry&65535)==x+1 && (bitmap[page*words+y/32]&(1u<<(y%32)));
    out[i]=pass?ushort(entry>>16):ushort(0);
}
"""
let options=MTLCompileOptions();options.fastMathEnabled=false
let library=try device.makeLibrary(source:source,options:options)
var pipes:[String:MTLComputePipelineState]=[:]
for name in ["powers","powers_packed","reconstruct","reconstruct_packed","decide","hash_filter"]{pipes[name]=try device.makeComputePipelineState(function:library.makeFunction(name:name)!)}
func buffer<T>(_ values:[T])->MTLBuffer{values.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}}
func empty(_ bytes:Int)->MTLBuffer{device.makeBuffer(length:bytes,options:.storageModeShared)!}
func matrix(_ b:MTLBuffer,_ rows:Int,_ cols:Int,_ type:MPSDataType)->MPSMatrix{MPSMatrix(buffer:b,descriptor:MPSMatrixDescriptor(rows:rows,columns:cols,rowBytes:cols*(type == .float16 ? 2:4),dataType:type))}
func kernel(_ cb:MTLCommandBuffer,_ name:String,_ bs:[MTLBuffer],_ configs:[[UInt32]],_ count:Int){
    let e=cb.makeComputeCommandEncoder()!;let pipe=pipes[name]!;e.setComputePipelineState(pipe)
    for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
    for (i,c) in configs.enumerated(){c.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count+i)}}
    e.dispatchThreads(MTLSize(width:count,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,pipe.maxTotalThreadsPerThreadgroup),height:1,depth:1));e.endEncoding()
}
let prepareStart=ProcessInfo.processInfo.systemUptime
func digitBuffers(_ values:ArraySlice<UInt32>)->(MTLBuffer,MTLBuffer){
    var low:[UInt16]=[],high:[UInt16]=[];low.reserveCapacity(values.count);high.reserveCapacity(values.count)
    for v in values{let signed=v>prime/2 ? Int(v)-prime:Int(v);let l=((signed+128)&255)-128;let h=(signed-l)/256
        low.append(Float16(l).bitPattern);high.append(Float16(h).bitPattern)}
    return(buffer(low),buffer(high))
}
let first=digitBuffers(coefficients[0..<2*m*k]),second=digitBuffers(coefficients[2*m*k..<3*m*k])
func stack(_ pair:(MTLBuffer,MTLBuffer))->MTLBuffer{
    let out=empty(pair.0.length+pair.1.length)
    memcpy(out.contents(),pair.0.contents(),pair.0.length)
    memcpy(out.contents().advanced(by:pair.0.length),pair.1.contents(),pair.1.length)
    return out
}
let firstStack=stack(first),secondStack=stack(second)
let qbuf=buffer(queries),table=buffer(hashTables),bitmap=buffer(bitmaps)
let result=empty(m*p*2),reference=empty(m*p*2),error=buffer([UInt32(0)])
let bl=empty(k*batch*2),bh=empty(k*batch*2)
let bpacked=empty(k*batch*4)
let products=(0..<(stacked ? 1:4)).map{_ in empty((stacked ? 8:2)*m*batch*4)}
let residues1=empty(2*m*batch*4),residues2=empty(m*batch*4)
let prepareSeconds=ProcessInfo.processInfo.systemUptime-prepareStart
var polyGPU=0.0
let polyStart=ProcessInfo.processInfo.systemUptime
for start in stride(from:0,to:p,by:batch){
    let count=min(batch,p-start),cb=queue.makeCommandBuffer()!
    for which in 0..<2{
        let rows=which==0 ? 2*m:m,co=which==0 ? first:second
        if stacked{
            kernel(cb,"powers_packed",[qbuf,bpacked],[[UInt32(start),UInt32(count),UInt32(k),UInt32(batch)],[UInt32(which)]],batch)
            let mul=MPSMatrixMultiplication(device:device,transposeLeft:false,transposeRight:false,resultRows:2*rows,resultColumns:2*batch,interiorColumns:k,alpha:1,beta:0)
            mul.encode(commandBuffer:cb,leftMatrix:matrix(which==0 ? firstStack:secondStack,2*rows,k,.float16),rightMatrix:matrix(bpacked,k,2*batch,.float16),resultMatrix:matrix(products[0],2*rows,2*batch,.float32))
            kernel(cb,"reconstruct_packed",[products[0],which==0 ? residues1:residues2],[[UInt32(rows),UInt32(batch)]],rows*batch)
        }else{
            kernel(cb,"powers",[qbuf,bl,bh],[[UInt32(start),UInt32(count),UInt32(k),UInt32(batch)],[UInt32(which)]],batch)
            let mul=MPSMatrixMultiplication(device:device,transposeLeft:false,transposeRight:false,resultRows:rows,resultColumns:batch,interiorColumns:k,alpha:1,beta:0)
            for (i,ab) in [(co.0,bl),(co.0,bh),(co.1,bl),(co.1,bh)].enumerated(){
                mul.encode(commandBuffer:cb,leftMatrix:matrix(ab.0,rows,k,.float16),rightMatrix:matrix(ab.1,k,batch,.float16),resultMatrix:matrix(products[i],rows,batch,.float32))
            }
            kernel(cb,"reconstruct",products+[which==0 ? residues1:residues2],[[UInt32(rows*batch)]],rows*batch)
        }
    }
    kernel(cb,"decide",[residues1,residues2,result,error],[[UInt32(m),UInt32(batch),UInt32(count),UInt32(start)],[UInt32(p)]],m*batch)
    cb.commit();cb.waitUntilCompleted();try require(cb.status == .completed,"matrix failed: \(String(describing:cb.error))")
    polyGPU+=cb.gpuEndTime-cb.gpuStartTime
}
let polyHost=ProcessInfo.processInfo.systemUptime-polyStart
try require(error.contents().assumingMemoryBound(to:UInt32.self).pointee==0,"invalid decoded handle")
let hashStart=ProcessInfo.processInfo.systemUptime,hash=queue.makeCommandBuffer()!
kernel(hash,"hash_filter",[qbuf,table,bitmap,reference],[[UInt32(m),UInt32(p),UInt32(capacity),UInt32(bitmapWords)]],m*p)
hash.commit();hash.waitUntilCompleted();try require(hash.status == .completed,"hash reference failed")
let hashHost=ProcessInfo.processInfo.systemUptime-hashStart,hashGPU=hash.gpuEndTime-hash.gpuStartTime
let rp=result.contents().assumingMemoryBound(to:UInt16.self),hp=reference.contents().assumingMemoryBound(to:UInt16.self)
var candidates=0
for i in 0..<m*p{if rp[i] != hp[i]{throw NSError(domain:"filter/payload mismatch at \(i)",code:1)};if rp[i] != 0{candidates+=1}}
// One same-device dense GEMM reference, not a hardware-peak or NVIDIA claim.
let n=1024,a=buffer([UInt16](repeating:Float16(1).bitPattern,count:n*n)),b=empty(n*n*4),cal=queue.makeCommandBuffer()!
let mm=MPSMatrixMultiplication(device:device,transposeLeft:false,transposeRight:false,resultRows:n,resultColumns:n,interiorColumns:n,alpha:1,beta:0)
mm.encode(commandBuffer:cal,leftMatrix:matrix(a,n,n,.float16),rightMatrix:matrix(a,n,n,.float16),resultMatrix:matrix(b,n,n,.float32))
cal.commit();cal.waitUntilCompleted();try require(cal.status == .completed,"calibration failed")
try Data(bytes:result.contents(),count:m*p*2).write(to:URL(fileURLWithPath:CommandLine.arguments[2]+".bin"))
let report=Report(device:device.name,layout:stacked ? "two stacked GEMMs; all digit quadrants used":"eight GEMMs",batch:batch,pages:m,coefficients:k,queries:p,polynomial_gpu_seconds:polyGPU,hash_gpu_seconds:hashGPU,
                  polynomial_host_submit_wait_seconds:polyHost,hash_host_submit_wait_seconds:hashHost,prepare_seconds:prepareSeconds,
                  exact_filter_and_handle_agreement:true,candidate_pages:candidates,
                  polynomial_float_operations:Double(2*4*3)*Double(m)*Double(k)*Double(p),
                  reference_gemm_seconds:cal.gpuEndTime-cal.gpuStartTime,reference_gemm_float_operations:Double(2)*Double(n*n*n),
                  scope:"standalone Metal/MPS operator; includes powers/GEMMs/modular reconstruction; no native inference speedup or NVIDIA Tensor Core claim")
try JSONEncoder().encode(report).write(to:URL(fileURLWithPath:CommandLine.arguments[2]+".json"))
