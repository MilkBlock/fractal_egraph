import Foundation
import Metal
func need(_ ok:Bool,_ why:String)throws{if !ok{throw NSError(domain:why,code:1)}}
let args=CommandLine.arguments
try need(args.count==5,"worker program.json kernel.metal output.bin metrics.json")
let p=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:args[1]))) as! [String:Any]
try need(p["schema"] as? String=="matrix-closure/v1","program schema")
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue() else{fatalError("Metal unavailable")}
let options=MTLCompileOptions();options.fastMathEnabled=false
let library=try device.makeLibrary(source:String(contentsOfFile:args[2],encoding:.utf8),options:options)
let mode=ProcessInfo.processInfo.environment["MATRIX_CLOSURE_MODE"] ?? "matrix"
try need(mode=="matrix" || mode=="direct","execution mode")
let names=["begin_run","gather_context","closure_gemm","publish","finish_run","direct_closure"]
let pipes=try names.map{try device.makeComputePipelineState(function:library.makeFunction(name:$0)!)}
func shared<T>(_ a:[T])->MTLBuffer{a.withUnsafeBytes{b in b.isEmpty ? device.makeBuffer(length:4,options:.storageModeShared)! : device.makeBuffer(bytes:b.baseAddress!,length:b.count,options:.storageModeShared)!}}
func priv(_ bytes:Int)->MTLBuffer{device.makeBuffer(length:max(4,bytes),options:.storageModePrivate)!}
let initial=(p["initial"] as! [NSNumber]).map{$0.uint32Value},count=initial.count
try need(count>0,"empty context")
try need(initial.allSatisfy{$0<=1},"separator requires Boolean state coordinates")
var state=priv(count*4),next=priv(count*4)
let birth=priv(count*4),meta=priv(20),status=device.makeBuffer(length:20,options:.storageModeShared)!
struct Bank {let config:[UInt32];let deps:MTLBuffer;let targets:MTLBuffer;let weights:MTLBuffer;let x:MTLBuffer;let scores:MTLBuffer;let threshold:Float}
var banks:[Bank]=[];var weightBytes=0,coordinateBytes=0,matrixOps:UInt64=0
for g in p["groups"] as! [[String:Any]]{
 let k=g["arity"] as! Int,targets=(g["targets"] as! [NSNumber]).map{$0.uint32Value}
 let n=targets.count,rows=(n+7)/8,width=8*(k+1),sep=g["separator"] as! [String:Any]
 let theta=(sep["coefficients"] as! [NSNumber]).map{$0.uint16Value}
 try need(theta.count==k+1,"coefficient shape")
 // Revalidate the actual FP16 coefficients before authorizing any GPU effect.
 let values=theta.map{Double(Float16(bitPattern:$0))},threshold=(sep["threshold"] as! NSNumber).doubleValue
 try need(values.allSatisfy{$0.isFinite} && threshold.isFinite,"finite separator")
 let pos=values.reduce(0,+),neg=k==0 ? pos-1 : pos-(values.dropFirst().min() ?? 0)
 let error=8*Double(Float.ulpOfOne)*values.map{abs($0)}.reduce(0,+)
 try need((k==0 || values.dropFirst().allSatisfy{$0>0}) && pos-threshold>error && threshold-neg>error,"uncertified ridge separator")
 var w=[UInt16](repeating:0,count:width*8)
 for block in 0..<8{for j in 0...k{w[(block*(k+1)+j)*8+block]=theta[j]}}
 let deps=(g["premises"] as! [[NSNumber]]).flatMap{$0.map{$0.uint32Value}}
 try need(deps.count==n*k && deps.allSatisfy{$0<UInt32(count)} && targets.allSatisfy{$0<UInt32(count)},"binding coordinates")
 banks.append(Bank(config:[UInt32(n),UInt32(k),UInt32(width),UInt32(rows)],deps:shared(deps),targets:shared(targets),weights:shared(w),x:priv(rows*width*2),scores:priv(n*4),threshold:Float(threshold)))
 weightBytes+=w.count*2;coordinateBytes+=rows*width*2
 matrixOps+=UInt64(((rows+7)/8)*8*8*width*2)
}
let initCommand=queue.makeCommandBuffer()!,blit=initCommand.makeBlitCommandEncoder()!
let initialBuffer=shared(initial),initialBirth=shared(initial.map{$0==1 ? UInt32(0):UInt32.max}),initialMeta=shared([UInt32(1),0,0,0,0])
blit.copy(from:initialBuffer,sourceOffset:0,to:state,destinationOffset:0,size:count*4)
blit.copy(from:initialBirth,sourceOffset:0,to:birth,destinationOffset:0,size:count*4)
blit.copy(from:initialMeta,sourceOffset:0,to:meta,destinationOffset:0,size:20)
for bank in banks{blit.fill(buffer:bank.x,range:0..<bank.x.length,value:0)}
blit.endEncoding();initCommand.commit();initCommand.waitUntilCompleted()
try need(initCommand.status == .completed,"initial upload")
func launch(_ cb:MTLCommandBuffer,_ index:Int,_ bs:[MTLBuffer],_ constants:[UInt32],_ threads:Int,_ grouped:Bool=false,_ threshold:Float?=nil){
 let e=cb.makeComputeCommandEncoder()!;e.setComputePipelineState(pipes[index]);e.label=names[index]
 for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
 constants.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count)}
 if var threshold=threshold{e.setBytes(&threshold,length:4,index:bs.count+1)}
 if grouped{e.dispatchThreadgroups(MTLSize(width:threads,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
 else{e.dispatchThreads(MTLSize(width:max(1,threads),height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(128,pipes[index].maxTotalThreadsPerThreadgroup),height:1,depth:1))}
 e.endEncoding()
}
let start=CFAbsoluteTimeGetCurrent();var gpuSeconds=0.0,batches=0,done:[UInt32]=[]
repeat{
 let cb=queue.makeCommandBuffer()!;cb.label="matrix closure batch"
 for _ in 0..<8{
  launch(cb,0,[state,next,meta],[UInt32(count)],count)
  for b in banks{
   if mode=="matrix"{
    launch(cb,1,[state,b.deps,b.x,meta],b.config,Int(b.config[0]))
    launch(cb,2,[b.x,b.weights,b.scores,meta],b.config,Int((b.config[3]+7)/8),true)
    launch(cb,3,[b.scores,b.targets,next,birth,meta],[b.config[0]],Int(b.config[0]),false,b.threshold)
   }else{launch(cb,5,[state,b.deps,b.targets,next,birth,meta],b.config,Int(b.config[0]))}
  }
  launch(cb,4,[meta],[UInt32(count)],1)
  swap(&state,&next)
 }
 let copy=cb.makeBlitCommandEncoder()!;copy.copy(from:meta,sourceOffset:0,to:status,destinationOffset:0,size:20);copy.endEncoding()
 cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"matrix iteration")
 gpuSeconds+=cb.gpuEndTime-cb.gpuStartTime;batches+=1
 done=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:5),count:5))
 try need(done[3]==0,"closure exceeded finite lattice height")
}while done[0] != 0
let seconds=CFAbsoluteTimeGetCurrent()-start
// Birth times reconstruct every visible Run snapshot from this one readback.
let final=device.makeBuffer(length:count*4,options:.storageModeShared)!,cb=queue.makeCommandBuffer()!,copy=cb.makeBlitCommandEncoder()!
copy.copy(from:birth,sourceOffset:0,to:final,destinationOffset:0,size:count*4);copy.endEncoding();cb.commit();cb.waitUntilCompleted()
try need(cb.status == .completed,"final readback")
try Data(bytes:final.contents(),count:count*4).write(to:URL(fileURLWithPath:args[3]))
let metrics:[String:Any]=["mode":mode,"gpu_seconds":gpuSeconds,"control_seconds":seconds,"runs":done[2],"new_facts":done[4],"batches":batches,"status_bytes":batches*20,"final_bytes":count*4,"weight_bytes":weightBytes,"coordinate_bytes":coordinateBytes,"resident_allocated_bytes":device.currentAllocatedSize,"matrix_flops_per_active_run":mode=="matrix" ? matrixOps : 0,"query_calls_during_iteration":0,"table_writes_during_iteration":0,"cpu_bindings_or_scores_during_iteration":0,"scope":"standalone compiled closure, not full Pass 3 runtime"]
try JSONSerialization.data(withJSONObject:metrics,options:[.prettyPrinted,.sortedKeys]).write(to:URL(fileURLWithPath:args[4]))
