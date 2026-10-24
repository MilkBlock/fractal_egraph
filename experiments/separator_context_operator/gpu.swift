// Exact projected-binding circuit. GPU combines relations and shares RHS keys;
// it receives factor matrices, not a CPU-generated list of complete matches.
import Foundation
import Metal
import MetalPerformanceShaders

struct Job:Decodable {let x:[[Float]];let y:[[Float]];let left:[[Float]];let types:[[Float]];let value_map:[[Float]];let normalization:Float?}
struct Input:Decodable {let jobs:[Job]}
struct Answer:Encodable {let compatible:[UInt32];let new_const:[UInt32]}
struct Report:Encodable {let device:String;let backend:String;let jobs:[Answer]}
let source="""
#include <metal_stdlib>
using namespace metal;
kernel void groups(device const float *l [[buffer(0)]],device const float *t [[buffer(1)]],
                   device float *g [[buffer(2)]],constant uint3 &sz [[buffer(3)]],uint id [[thread_position_in_grid]]){
    uint roots=sz.x,cs=sz.y,ts=sz.z;if(id>=roots*cs*ts)return;
    uint r=id/(cs*ts),c=(id/ts)%cs,k=id%ts;g[id]=float(l[r*cs+c]>0 && t[r*ts+k]>0);
}
kernel void mask(device const float *scores [[buffer(0)]],device const float *l [[buffer(1)]],
                 device const float *t [[buffer(2)]],device float *out [[buffer(3)]],
                 constant uint4 &sz [[buffer(4)]],uint id [[thread_position_in_grid]]){
    uint roots=sz.x,cols=sz.y,cs=sz.z,ts=sz.w;if(id>=roots*cols)return;
    uint r=id/cols;bool a=false,b=false;
    for(uint c=0;c<cs;c++)a=a || l[r*cs+c]>0;
    for(uint c=0;c<ts;c++)b=b || t[r*ts+c]>0;
    out[id]=float(scores[id]>0 && a && b);
}
"""
let input=try JSONDecoder().decode(Input.self,from:Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1])))
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue() else{fatalError("Metal unavailable")}
let options=MTLCompileOptions();options.fastMathEnabled=false
let lib=try device.makeLibrary(source:source,options:options)
let gpipe=try device.makeComputePipelineState(function:lib.makeFunction(name:"groups")!)
let mpipe=try device.makeComputePipelineState(function:lib.makeFunction(name:"mask")!)
func buffer(_ a:[Float])->MTLBuffer{a.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}}
func empty(_ count:Int)->MTLBuffer{device.makeBuffer(length:count*4,options:.storageModeShared)!}
func mat(_ b:MTLBuffer,_ rows:Int,_ cols:Int)->MPSMatrix{MPSMatrix(buffer:b,descriptor:MPSMatrixDescriptor(rows:rows,columns:cols,rowBytes:cols*4,dataType:.float32))}
func dispatch(_ cb:MTLCommandBuffer,_ pipe:MTLComputePipelineState,_ bs:[MTLBuffer],_ cfg:[UInt32],_ count:Int){
    let e=cb.makeComputeCommandEncoder()!;e.setComputePipelineState(pipe)
    for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
    cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count)}
    e.dispatchThreads(MTLSize(width:count,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,pipe.maxTotalThreadsPerThreadgroup),height:1,depth:1));e.endEncoding()
}
var answers:[Answer]=[]
for j in input.jobs{
    let r=j.x.count,c=j.y.count,k=j.x[0].count,cs=j.left[0].count,ts=j.types[0].count,v=j.value_map[0].count,g=cs*ts
    let x=buffer(j.x.flatMap{$0}),y=buffer(j.y.flatMap{$0}),l=buffer(j.left.flatMap{$0}),t=buffer(j.types.flatMap{$0}),vm=buffer(j.value_map.flatMap{$0})
    let scores=empty(r*c),valid=empty(r*c),groups=empty(r*g),rv=empty(r*v),newConst=empty(g*v)
    let cb=queue.makeCommandBuffer()!
    let join=MPSMatrixMultiplication(device:device,transposeLeft:false,transposeRight:true,resultRows:r,resultColumns:c,interiorColumns:k,alpha:1.0/Double(j.normalization ?? Float(k)),beta:0)
    join.encode(commandBuffer:cb,leftMatrix:mat(x,r,k),rightMatrix:mat(y,c,k),resultMatrix:mat(scores,r,c))
    dispatch(cb,mpipe,[scores,l,t,valid],[UInt32(r),UInt32(c),UInt32(cs),UInt32(ts)],r*c)
    // uint3 has 16-byte argument alignment in MSL.
    dispatch(cb,gpipe,[l,t,groups],[UInt32(r),UInt32(cs),UInt32(ts),0],r*g)
    let project=MPSMatrixMultiplication(device:device,transposeLeft:false,transposeRight:false,resultRows:r,resultColumns:v,interiorColumns:c,alpha:1,beta:0)
    project.encode(commandBuffer:cb,leftMatrix:mat(valid,r,c),rightMatrix:mat(vm,c,v),resultMatrix:mat(rv,r,v))
    let share=MPSMatrixMultiplication(device:device,transposeLeft:true,transposeRight:false,resultRows:g,resultColumns:v,interiorColumns:r,alpha:1,beta:0)
    share.encode(commandBuffer:cb,leftMatrix:mat(groups,r,g),rightMatrix:mat(rv,r,v),resultMatrix:mat(newConst,g,v))
    cb.commit();cb.waitUntilCompleted();if cb.status != .completed{fatalError("\(String(describing:cb.error))")}
    func flags(_ b:MTLBuffer,_ n:Int)->[UInt32]{let p=b.contents().assumingMemoryBound(to:Float.self);return (0..<n).map{p[$0]>0 ? 1:0}}
    answers.append(Answer(compatible:flags(valid,r*c),new_const:flags(newConst,g*v)))
}
let report=Report(device:device.name,backend:"MPS signed-separator GEMM + masks + two RHS-projection GEMMs; no performance claim",jobs:answers)
try JSONEncoder().encode(report).write(to:URL(fileURLWithPath:CommandLine.arguments[2]))
