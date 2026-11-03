// Exact shared-context response: dense GEMM vs sparse predicate evaluation.
// This consumes prepared features, NOT native egglog tables or a live journal.
import Foundation
import Metal
import MetalPerformanceShaders

func require(_ b:Bool,_ s:String)throws{if !b{throw NSError(domain:s,code:1)}}
try require(CommandLine.arguments.count==3,"usage: worker input.bin output.json")
let input=try Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1]))
try require(String(data:input.prefix(8),encoding:.utf8)=="RESPBNK2","bad input header")
var cursor=8
func uints(_ n:Int)->[UInt32]{let start=cursor;cursor+=n*4;precondition(cursor<=input.count);return input.withUnsafeBytes{r in (0..<n).map{r.loadUnaligned(fromByteOffset:start+4*$0,as:UInt32.self)}}}
func halves(_ n:Int)->[UInt16]{let start=cursor;cursor+=n*2;precondition(cursor<=input.count);return input.withUnsafeBytes{r in (0..<n).map{r.loadUnaligned(fromByteOffset:start+2*$0,as:UInt16.self)}}}
func bytes(_ n:Int)->[UInt8]{defer{cursor+=n};return Array(input[cursor..<cursor+n])}
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue()else{fatalError("no Metal device")}
func buffer<T>(_ a:[T])->MTLBuffer{
    if a.isEmpty{return device.makeBuffer(length:4,options:.storageModeShared)!}
    return a.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}
}
func zero(_ n:Int)->MTLBuffer{let b=device.makeBuffer(length:max(4,n),options:.storageModeShared)!;memset(b.contents(),0,b.length);return b}
func pad(_ n:Int)->Int{max(8,(n+7)/8*8)}
func matrix(_ a:[UInt16],_ rows:Int,_ cols:Int)->MPSMatrix{
    let stride=pad(cols);var v=[UInt16](repeating:0,count:max(1,rows)*stride)
    for r in 0..<rows{for c in 0..<cols{v[r*stride+c]=a[r*cols+c]}}
    return MPSMatrix(buffer:buffer(v),descriptor:MPSMatrixDescriptor(rows:max(1,rows),columns:max(1,cols),rowBytes:stride*2,dataType:.float16))
}
struct Job {
    let n:Int,k:Int,q:Int,p:Int,f:Int,g:Int
    let x:MPSMatrix,w:MPSMatrix,z:MPSMatrix,fw:MPSMatrix
    let route:MTLBuffer,groute:MTLBuffer,t:MTLBuffer,ft:MTLBuffer
    let offsets:MTLBuffer,indices:MTLBuffer
    let output:MTLBuffer,rawScore:MPSMatrix,foldScore:MPSMatrix,expected:[UInt8]
}
var jobs:[Job]=[]
let count=Int(uints(1)[0])
for _ in 0..<count{
    let d=uints(6).map(Int.init),n=d[0],k=d[1],q=d[2],p=d[3],f=d[4],g=d[5]
    let x=halves(n*k),w=halves(k*q),z=halves(p*f),fw=halves(f*g)
    let route=uints(n),groute=uints(q),t=uints(q),ft=uints(g),expected=bytes(n*q)
    var offsets:[UInt32]=[0],indices:[UInt32]=[]
    for j in 0..<q{for i in 0..<k{if Float16(bitPattern:w[i*q+j]) != 0{indices.append(UInt32(i))}};offsets.append(UInt32(indices.count))}
    let raw=MPSMatrix(buffer:zero(n*pad(q)*4),descriptor:MPSMatrixDescriptor(rows:n,columns:q,rowBytes:pad(q)*4,dataType:.float32))
    let folded=MPSMatrix(buffer:zero(p*pad(g)*4),descriptor:MPSMatrixDescriptor(rows:p,columns:g,rowBytes:pad(g)*4,dataType:.float32))
    jobs.append(Job(n:n,k:k,q:q,p:p,f:f,g:g,x:matrix(x,n,k),w:matrix(w,k,q),z:matrix(z,p,f),fw:matrix(fw,f,g),route:buffer(route),groute:buffer(groute),t:buffer(t),ft:buffer(ft),offsets:buffer(offsets),indices:buffer(indices),output:zero(n*q),rawScore:raw,foldScore:folded,expected:expected))
}
try require(cursor==input.count,"trailing packet bytes")
let source="""
#include <metal_stdlib>
using namespace metal;
kernel void raw_read(device const float*score[[buffer(0)]],device const uint*t[[buffer(1)]],device uchar*out[[buffer(2)]],constant uint4&c[[buffer(3)]],uint i[[thread_position_in_grid]]){
    if(i>=c.x*c.y)return;out[i]=score[(i/c.y)*c.z+i%c.y]==float(t[i%c.y]);
}
kernel void fold_read(device const float*score[[buffer(0)]],device const uint*t[[buffer(1)]],device const uint*route[[buffer(2)]],device const uint*groute[[buffer(3)]],device uchar*out[[buffer(4)]],constant uint4&c[[buffer(5)]],uint i[[thread_position_in_grid]]){
    if(i>=c.x*c.y)return;uint r=route[i/c.y],q=groute[i%c.y];out[i]=score[r*c.z+q]==float(t[q]);
}
kernel void raw_sparse(device const half*x[[buffer(0)]],device const uint*offset[[buffer(1)]],device const uint*indices[[buffer(2)]],device uchar*out[[buffer(3)]],constant uint4&c[[buffer(4)]],uint i[[thread_position_in_grid]]){
    if(i>=c.x*c.y)return;uint row=i/c.y,q=i%c.y;bool yes=true;
    for(uint j=offset[q];j<offset[q+1];j++){if(x[row*c.z+indices[j]]==half(0)){yes=false;break;}}out[i]=yes;
}
kernel void fold_integer(device const half*z[[buffer(0)]],device const half*w[[buffer(1)]],device float*out[[buffer(2)]],constant uint4&c[[buffer(3)]],constant uint2&s[[buffer(4)]],uint i[[thread_position_in_grid]]){
    if(i>=c.x*c.y)return;uint row=i/c.y,q=i%c.y,total=0;
    for(uint j=0;j<c.z;j++){total+=uint(z[row*c.w+j])*uint(w[j*s.x+q]);}out[row*s.y+q]=float(total);
}
"""
let options=MTLCompileOptions();options.fastMathEnabled=false
let lib=try device.makeLibrary(source:source,options:options)
var pipes:[String:MTLComputePipelineState]=[:]
for s in ["raw_read","fold_read","raw_sparse","fold_integer"]{pipes[s]=try device.makeComputePipelineState(function:lib.makeFunction(name:s)!)}
func dispatch(_ cb:MTLCommandBuffer,_ name:String,_ bs:[MTLBuffer],_ cs:[[UInt32]],_ count:Int){
    let e=cb.makeComputeCommandEncoder()!,p=pipes[name]!;e.setComputePipelineState(p)
    for(i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
    for(i,c) in cs.enumerated(){c.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count+i)}}
    e.dispatchThreads(MTLSize(width:count,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,p.maxTotalThreadsPerThreadgroup),height:1,depth:1));e.endEncoding()
}
func execute(_ mode:String)throws->[String:Double]{
    let host=ProcessInfo.processInfo.systemUptime,cb=queue.makeCommandBuffer()!
    for j in jobs{
        if mode=="raw_gemm"{
            let mul=MPSMatrixMultiplication(device:device,transposeLeft:false,transposeRight:false,resultRows:j.n,resultColumns:j.q,interiorColumns:j.k,alpha:1,beta:0)
            mul.encode(commandBuffer:cb,leftMatrix:j.x,rightMatrix:j.w,resultMatrix:j.rawScore)
            dispatch(cb,"raw_read",[j.rawScore.data,j.t,j.output],[[UInt32(j.n),UInt32(j.q),UInt32(pad(j.q)),0]],j.n*j.q)
        }else if mode=="raw_sparse"{
            dispatch(cb,"raw_sparse",[j.x.data,j.offsets,j.indices,j.output],[[UInt32(j.n),UInt32(j.q),UInt32(pad(j.k)),0]],j.n*j.q)
        }else{
            if j.f>0{
                if mode=="shared_gemm"{
                    let mul=MPSMatrixMultiplication(device:device,transposeLeft:false,transposeRight:false,resultRows:j.p,resultColumns:j.g,interiorColumns:j.f,alpha:1,beta:0)
                    mul.encode(commandBuffer:cb,leftMatrix:j.z,rightMatrix:j.fw,resultMatrix:j.foldScore)
                }else{
                    dispatch(cb,"fold_integer",[j.z.data,j.fw.data,j.foldScore.data],[[UInt32(j.p),UInt32(j.g),UInt32(j.f),UInt32(pad(j.f))],[UInt32(pad(j.g)),UInt32(pad(j.g))]],j.p*j.g)
                }
            }
            dispatch(cb,"fold_read",[j.foldScore.data,j.ft,j.route,j.groute,j.output],[[UInt32(j.n),UInt32(j.q),UInt32(pad(j.g)),0]],j.n*j.q)
        }
    }
    cb.commit();cb.waitUntilCompleted();try require(cb.status == .completed,"GPU command failure: \(String(describing:cb.error))")
    let wall=ProcessInfo.processInfo.systemUptime-host
    for j in jobs{
        let p=j.output.contents().assumingMemoryBound(to:UInt8.self)
        for i in 0..<j.n*j.q{try require(p[i]==j.expected[i],"incorrect \(mode) root-rule response")}
    }
    return ["gpu_seconds":cb.gpuEndTime-cb.gpuStartTime,"host_encode_submit_wait_seconds":wall]
}
let modes=["raw_gemm","raw_sparse","shared_gemm","shared_integer"]
// Warm each implementation once, then retain ONE timed execution of each.
for mode in modes{_ = try execute(mode)}
var timings:[String:[String:Double]]=[:]
for mode in modes{timings[mode]=try execute(mode)}
let report:[String:Any]=["device":device.name,"jobs":jobs.count,"exact_all_four_implementations":true,
    "timing":timings,"warmups_per_mode":1,"retained_runs_per_mode":1,
    "gemms_raw":jobs.count,"gemms_after_cofactor":jobs.filter{$0.f>0}.count,
    "root_rule_outputs":jobs.reduce(0){$0+$1.n*$1.q},
    "scope":"prepared feature response only; includes decoding back to original rows; excludes feature reads, indices, CPU quotient, transfer, RHS and native inference; Apple MPS, not NVIDIA Tensor Core evidence"]
let result=try JSONSerialization.data(withJSONObject:report,options:[.prettyPrinted,.sortedKeys])
try result.write(to:URL(fileURLWithPath:CommandLine.arguments[2]));print(String(data:result,encoding:.utf8)!)
