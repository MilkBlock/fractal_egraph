// Correctness backend for the manually derived coordinate representation.
// MPS GEMM + elementwise state update. One upload / one command buffer; no CPU
// binding search between rounds. Union requests are returned, not committed.
import Foundation
import Metal
import MetalPerformanceShaders

struct Epoch: Decodable { let t:[Float]; let h:[Int32]; let root_h:[Int32]; let known:[UInt32] }
struct Job: Decodable {
    let n:Int; let slots:Int; let width:Int; let nodes:[UInt32]; let seeds:[UInt32]; let lengths:[Int32]; let single:[UInt32]; let opaque:[UInt32]
    let gets:[UInt32]; let epochs:[Epoch]; let steps:[Int]
}
struct State: Encodable { let a:[Float]; let frontier:[Float]; let gets:[UInt32]; let single_effect:[UInt32]; let opaque_effect:[UInt32] }
struct Report: Encodable { let device:String; let backend:String; let rounds:[State]; let command_buffers:Int; let cpu_interround_readbacks:Int }
func require(_ x:Bool,_ msg:String) throws {if !x{throw NSError(domain:msg,code:1)}}

let source="""
#include <metal_stdlib>
using namespace metal;
kernel void advance(device const float *old [[buffer(0)]],
                    device const float *product [[buffer(1)]],
                    device const float *front [[buffer(2)]],
                    device const uint *gets [[buffer(3)]],
                    device float *next [[buffer(4)]],
                    device float *nf [[buffer(5)]],
                    device atomic_uint *ng [[buffer(6)]],
                    device const int *h [[buffer(7)]],
                    device const int *lengths [[buffer(8)]],
                    device const uint *known [[buffer(9)]],
                    device const uint *single [[buffer(10)]],
                    device const uint *opaque [[buffer(11)]],
                    device uint *singleEffect [[buffer(12)]],
                    device uint *opaqueEffect [[buffer(13)]],
                    device atomic_uint *error [[buffer(14)]],
                    constant uint3 &size [[buffer(15)]],
                    device const int *rootH [[buffer(16)]],
                    device const uint *nodes [[buffer(17)]],
                    device const uint *seeds [[buffer(18)]], uint id [[thread_position_in_grid]]) {
    uint n=size.x,s=size.y,w=size.z;if(id>=n*s)return;
    uint r=id/s,j=id%s;
    float a=old[id];float value=min(1.0f,a+product[id]);
    if(seeds[r]==j){for(uint i=0;i<w;i++)if(gets[r*w+i])value=1.0f;}
    next[id]=value;
    bool step=front[id]>=0 && known[j] && front[id]<float(lengths[j]);
    nf[id]=max(front[id]+float(step),(a>0 && opaque[j])?0.0f:-1.0f);
    singleEffect[id]=uint(a>0 && single[j]);opaqueEffect[id]=uint(step);
    if(seeds[r]==j && single[j])atomic_store_explicit(&ng[r*w],1,memory_order_relaxed);
    if(singleEffect[id] || step){
        int offset=h[j]-rootH[r];int p=step?int(front[id]):0;int index=offset+p;
        if(index<0 || index>=int(w) || p<0 || p>=int(w)){
            atomic_store_explicit(error,1,memory_order_relaxed);return;
        }
        atomic_store_explicit(&ng[r*w+uint(index)],1,memory_order_relaxed);
        if(step)atomic_store_explicit(&ng[nodes[j]*w+uint(p)],1,memory_order_relaxed);
    }
}
"""
let job=try JSONDecoder().decode(Job.self,from:Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1])))
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue() else{fatalError("Metal unavailable")}
let n=job.n,s=job.slots,w=job.width
try require(n>0 && n<1<<24 && s>0 && s<1<<24 && w>0 && w<1<<24,"FP32 exact integer contract")
try require(job.lengths.allSatisfy{$0>0 && $0<1<<24},"length range")
func buffer<T>(_ xs:[T])->MTLBuffer{xs.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}}
func empty(_ bytes:Int)->MTLBuffer{device.makeBuffer(length:bytes,options:.storageModeShared)!}
func matrix(_ b:MTLBuffer,_ rows:Int,_ cols:Int)->MPSMatrix{MPSMatrix(buffer:b,descriptor:MPSMatrixDescriptor(rows:rows,columns:cols,rowBytes:cols*4,dataType:.float32))}
var a=buffer([Float](repeating:0,count:n*s)),next=empty(n*s*4)
var f=buffer([Float](repeating:-1,count:n*s)),nf=empty(n*s*4)
var g=buffer(job.gets),ng=empty(n*w*4)
let product=empty(n*s*4),lengths=buffer(job.lengths),single=buffer(job.single),opaque=buffer(job.opaque)
let nodes=buffer(job.nodes),seeds=buffer(job.seeds)
let error=buffer([UInt32(0)])
let epochs=job.epochs.map{(matrix(buffer($0.t),s,s),buffer($0.h),buffer($0.known),buffer($0.root_h))}
let options=MTLCompileOptions();options.fastMathEnabled=false
let library=try device.makeLibrary(source:source,options:options)
let kernel=try device.makeComputePipelineState(function:library.makeFunction(name:"advance")!)
let gemm=MPSMatrixMultiplication(device:device,transposeLeft:false,transposeRight:false,resultRows:n,resultColumns:s,interiorColumns:s,alpha:1,beta:0)
let cb=queue.makeCommandBuffer()!
var traces:[(MTLBuffer,MTLBuffer,MTLBuffer,MTLBuffer,MTLBuffer)]=[]
for epochIndex in job.steps {
    let epoch=epochs[epochIndex]
    gemm.encode(commandBuffer:cb,leftMatrix:matrix(a,n,s),rightMatrix:epoch.0,resultMatrix:matrix(product,n,s))
    let copy=cb.makeBlitCommandEncoder()!;copy.copy(from:g,sourceOffset:0,to:ng,destinationOffset:0,size:n*w*4);copy.endEncoding()
    let se=empty(n*s*4),oe=empty(n*s*4)
    let encoder=cb.makeComputeCommandEncoder()!;encoder.setComputePipelineState(kernel)
    let buffers=[a,product,f,g,next,nf,ng,epoch.1,lengths,epoch.2,single,opaque,se,oe,error]
    for (index,b) in buffers.enumerated(){encoder.setBuffer(b,offset:0,index:index)}
    encoder.setBuffer(epoch.3,offset:0,index:16);encoder.setBuffer(nodes,offset:0,index:17);encoder.setBuffer(seeds,offset:0,index:18)
    var cfg=SIMD3<UInt32>(UInt32(n),UInt32(s),UInt32(w));encoder.setBytes(&cfg,length:MemoryLayout.size(ofValue:cfg),index:15)
    let width=min(kernel.maxTotalThreadsPerThreadgroup,256)
    encoder.dispatchThreads(MTLSize(width:n*s,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:width,height:1,depth:1));encoder.endEncoding()
    let ta=empty(n*s*4),tf=empty(n*s*4),tg=empty(n*w*4)
    let save=cb.makeBlitCommandEncoder()!
    for (src,dst,bytes) in [(next,ta,n*s*4),(nf,tf,n*s*4),(ng,tg,n*w*4)]{save.copy(from:src,sourceOffset:0,to:dst,destinationOffset:0,size:bytes)}
    save.endEncoding();traces.append((ta,tf,tg,se,oe))
    swap(&a,&next);swap(&f,&nf);swap(&g,&ng)
}
cb.commit();cb.waitUntilCompleted()
try require(cb.status == .completed,"GPU command failed: \(String(describing:cb.error))")
try require(error.contents().assumingMemoryBound(to:UInt32.self).pointee==0,"coordinate bounds error")
func read<T>(_ b:MTLBuffer,_ t:T.Type,_ count:Int)->[T]{Array(UnsafeBufferPointer(start:b.contents().assumingMemoryBound(to:t),count:count))}
let states=traces.map{State(a:read($0.0,Float.self,n*s),frontier:read($0.1,Float.self,n*s),gets:read($0.2,UInt32.self,n*w),single_effect:read($0.3,UInt32.self,n*s),opaque_effect:read($0.4,UInt32.self,n*s))}
let report=Report(device:device.name,backend:"resident FP32 MPS GEMM plus Metal masks/frontiers; deferred union packets",rounds:states,command_buffers:1,cpu_interround_readbacks:0)
let encoder=JSONEncoder();try encoder.encode(report).write(to:URL(fileURLWithPath:CommandLine.arguments[2]))
