// Batch transport for independent typed Cell programs. CPU never executes a
// stage, joins bindings, computes a guard, or supplies an inferred endpoint.
import Foundation
import Metal

func require(_ ok:Bool,_ message:String)throws {if !ok {throw NSError(domain:message,code:1)}}
let input=URL(fileURLWithPath:CommandLine.arguments[1]), output=URL(fileURLWithPath:CommandLine.arguments[2])
let program=try JSONSerialization.jsonObject(with:Data(contentsOf:input)) as! [String:Any]
let spec=program["generic_cell"] as! [String:Any],layout=spec["layout"] as! [String:Int]
let transportVersion=spec["transport_version"] as? Int ?? 1
try require(transportVersion==1 || transportVersion==2 || transportVersion==7,"unknown local worker transport version")
let instances=program["instances"] as! [[String:Any]],n=instances.count
let device=MTLCreateSystemDefaultDevice()!,queue=device.makeCommandQueue()!
let options=MTLCompileOptions();options.fastMathEnabled=false
let compileStart=CFAbsoluteTimeGetCurrent()
let lib=try device.makeLibrary(source:program["shader"] as! String,options:options)
let pipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:spec["kernel"] as? String ?? "generic_cell_advance")!)
let batchEval:MTLComputePipelineState?=transportVersion==7 ? try device.makeComputePipelineState(function:lib.makeFunction(name:"batched_cell_evaluate")!):nil
let batchPublish:MTLComputePipelineState?=transportVersion==7 ? try device.makeComputePipelineState(function:lib.makeFunction(name:"batched_cell_publish")!):nil
let batchMatch:MTLComputePipelineState?=transportVersion==7 ? try device.makeComputePipelineState(function:lib.makeFunction(name:"batched_cell_match")!):nil
let batchPack:MTLComputePipelineState?=transportVersion==7 ? try device.makeComputePipelineState(function:lib.makeFunction(name:"batched_cell_pack")!):nil
let hashPipeline:MTLComputePipelineState?=transportVersion==7 ? try device.makeComputePipelineState(function:lib.makeFunction(name:"batched_cell_fingerprint")!):nil
let hashFinish:MTLComputePipelineState?=transportVersion==7 ? try device.makeComputePipelineState(function:lib.makeFunction(name:"batched_cell_fingerprint_finish")!):nil
let compileSeconds=CFAbsoluteTimeGetCurrent()-compileStart
func zeros(_ bytes:Int)->MTLBuffer {let b=device.makeBuffer(length:max(8,bytes),options:.storageModeShared)!;memset(b.contents(),0,b.length);return b}
func copy<T>(_ x:[T],_ b:MTLBuffer,_ offset:Int=0){x.withUnsafeBytes{v in if !v.isEmpty {memcpy(b.contents().advanced(by:offset),v.baseAddress!,v.count)}}}
func u32(_ b:MTLBuffer,_ offset:Int,_ count:Int)->[UInt32]{Array(UnsafeBufferPointer(start:b.contents().advanced(by:offset*4).bindMemory(to:UInt32.self,capacity:count),count:count))}
func i64(_ b:MTLBuffer,_ offset:Int,_ count:Int)->[Int64]{Array(UnsafeBufferPointer(start:b.contents().advanced(by:offset*8).bindMemory(to:Int64.self,capacity:count),count:count))}
let rowWords=layout["row_words"]!,uintWords=layout["uint_words"]!,longWords=layout["long_words"]!,stateWords=layout["state_words"]!
let rows=zeros(n*rowWords*8),uints=zeros(n*uintWords*4),longs=zeros(n*longWords*8),metas=zeros(n*36),states=zeros(n*stateWords*4)
let ids=program["id_capacity"] as! Int,capacity=program["capacity"] as! Int,stride=program["stride"] as! Int
let unionCap=max(capacity,(program["match_capacity"] as! Int)*2)
for (i,frame) in instances.enumerated(){
 copy((frame["initial_rows"] as! [NSNumber]).map{$0.int64Value},rows,i*rowWords*8)
 copy((frame["initial_sets"] as! [NSNumber]).map{$0.int64Value},rows,(i*rowWords+capacity*stride)*8)
 copy((frame["meta"] as! [NSNumber]).map{$0.uint32Value},metas,i*36)
 for name in ["parent","fp"]{copy((0..<ids).map{UInt32($0)},uints,(i*uintWords+layout[name]!)*4)}
 // Conservative flags cost only local index rebuilds, never miss a merge key.
 copy([UInt32](repeating:1,count:ids),uints,(i*uintWords+layout["parent"]!+ids+3+2*unionCap)*4)
}
let actions=(program["action_code"] as! [NSNumber]).map{$0.int64Value},code=zeros(actions.count*8);copy(actions,code)
let control=spec["control"] as! [String:Any]
let instructions=(control["instructions"] as! [[NSNumber]]).flatMap{$0.map{$0.uint32Value}}
let tape=zeros(instructions.count*4);copy(instructions,tape)
let historyWidth=spec["history_width"] as? Int ?? 8
let batchLayout=program["batch_layout"] as? [String:Any]
let batchHeader=batchLayout?["stats_words"] as? Int ?? 24
let batchStride=batchLayout?["stride"] as? Int ?? 1
var batchCapacity=128,batchWords=transportVersion==7 ? batchHeader+128*(9+batchStride):1
var batchData=zeros(n*batchWords*8),batchGrowths=0
if transportVersion==7 && program["batch_audit_numeric"] as? Bool == true {for i in 0..<n {copy([Int64(1)],batchData,(i*batchWords+14)*8)}}
let batchDispatch=zeros(36);copy([UInt32(0),UInt32(n),UInt32(1),UInt32(0),UInt32(n),UInt32(1),UInt32(0),UInt32(n),UInt32(1)],batchDispatch)
let hashChunks=(capacity*stride+2*ids+(program["set_words"] as! Int)+1023)/1024
let hashData=zeros(transportVersion==7 ? n*hashChunks*16:8)
var historyCapacity=16,history=zeros(n*16*historyWidth*4),dispatches=0,gpuSeconds=0.0
// Growth description is a string, so read numeric fields individually.
let cacheProgram=program["region_cache"] as? [String:Any]
let cacheHeader=cacheProgram?["header_words"] as? Int ?? 0
var cacheWords=cacheProgram?["initial_words"] as? Int ?? 1
var markerCache=zeros(n*cacheWords*8),cacheGrowths=0
if cacheHeader>0 {for i in 0..<n {copy([Int64(cacheHeader),Int64(cacheWords)],markerCache,i*cacheWords*8)}}
let quantum=UInt32(program["quantum"] as? Int ?? 8)
let start=CFAbsoluteTimeGetCurrent()
var lastProgress=start
while true {
 var pending=false,needGrowth=false,needCacheGrowth=false,needBatchGrowth=false
 for i in 0..<n {
  let s=u32(states,i*stateWords,8),m=u32(metas,i*9,9)
  if s[2]==0 && (m[4]==0 || (cacheHeader>0 && m[4]==31) || (transportVersion==7 && m[4]==34)) {
   pending=true;if s[4] != 0 {needGrowth=true};if m[4]==31 {needCacheGrowth=true};if m[4]==34 {needBatchGrowth=true}
  }
 }
 if !pending {break}
 if needCacheGrowth {
  try require(cacheWords <= Int(UInt32.max)/2, "region cache exceeds device address space")
  let next=zeros(n*cacheWords*2*8)
  for i in 0..<n {
   memcpy(next.contents().advanced(by:i*cacheWords*2*8),markerCache.contents().advanced(by:i*cacheWords*8),cacheWords*8)
   copy([Int64(cacheWords*2)],next,(i*cacheWords*2+1)*8)
   if u32(metas,i*9+4,1)[0]==31 {copy([UInt32(0)],metas,(i*9+4)*4)}
  }
  markerCache=next;cacheWords*=2;cacheGrowths+=1
 }
 if needBatchGrowth {
  let newCapacity=batchCapacity*2,newWords=batchHeader+newCapacity*(9+batchStride),next=zeros(n*newWords*8)
  try require(newWords<Int(UInt32.max),"batch context exceeds device address space")
  for i in 0..<n {
   memcpy(next.contents().advanced(by:i*newWords*8),batchData.contents().advanced(by:i*batchWords*8),batchHeader*8)
   let oldRecords=batchHeader+batchCapacity*(5+batchStride),newRecords=batchHeader+newCapacity*(5+batchStride)
   memcpy(next.contents().advanced(by:(i*newWords+newRecords)*8),batchData.contents().advanced(by:(i*batchWords+oldRecords)*8),4*batchCapacity*8)
   copy([Int64(newCapacity)],next,(i*newWords+9)*8)
   if u32(metas,i*9+4,1)[0]==34 {copy([UInt32(0)],metas,(i*9+4)*4)}
  }
  batchData=next;batchWords=newWords;batchCapacity=newCapacity;batchGrowths+=1
 }
 if needGrowth {
  let next=zeros(n*historyCapacity*2*historyWidth*4)
  for i in 0..<n {memcpy(next.contents().advanced(by:i*historyCapacity*2*historyWidth*4),history.contents().advanced(by:i*historyCapacity*historyWidth*4),historyCapacity*historyWidth*4);copy([UInt32(0)],states,(i*stateWords+4)*4)}
  history=next;historyCapacity*=2
 }
 let cb=queue.makeCommandBuffer()!
 cb.label=transportVersion==7 ? "shared-context multi-stage batch":"generic Cell local schedule"
 func bind(_ e:MTLComputeCommandEncoder){
  let buffers=[rows,uints,longs,metas,states,code,tape,history]+(transportVersion>=2 ? [markerCache]:[])
  for (i,b) in buffers.enumerated(){e.setBuffer(b,offset:0,index:i)}
  var cfg=SIMD4<UInt32>(UInt32(n),quantum,UInt32(historyCapacity),UInt32(cacheWords))
  e.setBytes(&cfg,length:16,index:transportVersion>=2 ? 9:8)
  if transportVersion==7 {
   e.setBuffer(batchData,offset:0,index:10)
   var batchCfg=SIMD4<UInt32>(UInt32(batchCapacity),0,0,0);e.setBytes(&batchCfg,length:16,index:11)
   e.setBuffer(batchDispatch,offset:0,index:12);e.setBuffer(hashData,offset:0,index:13)
  }
 }
 if transportVersion==7 {
  try require(batchEval!.maxTotalThreadsPerThreadgroup>=128,"batch evaluator requires 128 lanes")
  for _ in 0..<quantum {
   let clear=cb.makeBlitCommandEncoder()!;clear.fill(buffer:batchDispatch,range:0..<4,value:0);clear.fill(buffer:batchDispatch,range:12..<16,value:0);clear.fill(buffer:batchDispatch,range:24..<28,value:0);clear.endEncoding()
   let e=cb.makeComputeCommandEncoder()!;bind(e)
   e.setComputePipelineState(pipeline);e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(32,pipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   e.setComputePipelineState(batchMatch!);e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:24,threadsPerThreadgroup:MTLSize(width:128,height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   e.setComputePipelineState(batchPack!);e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(32,batchPack!.maxTotalThreadsPerThreadgroup),height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   e.setComputePipelineState(batchEval!);e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:0,threadsPerThreadgroup:MTLSize(width:128,height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   e.setComputePipelineState(batchPublish!);e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(32,batchPublish!.maxTotalThreadsPerThreadgroup),height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   e.setComputePipelineState(hashPipeline!);e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12,threadsPerThreadgroup:MTLSize(width:128,height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   e.setComputePipelineState(hashFinish!);e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(32,hashFinish!.maxTotalThreadsPerThreadgroup),height:1,depth:1))
   e.endEncoding()
  }
 }else{
  let e=cb.makeComputeCommandEncoder()!;bind(e);e.setComputePipelineState(pipeline)
  e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(32,pipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1));e.endEncoding()
 }
 cb.commit();cb.waitUntilCompleted();try require(cb.status == .completed,"local Cell GPU command failed: \(String(describing:cb.error))")
 gpuSeconds+=cb.gpuEndTime-cb.gpuStartTime;dispatches+=1
 if CFAbsoluteTimeGetCurrent()-lastProgress>15 {
  let m=u32(metas,0,9),s=u32(states,0,8)
  FileHandle.standardError.write(Data("[resident] round \(m[6]) pc \(s[0]) rows \(m[0]) error \(m[4])\n".utf8));lastProgress=CFAbsoluteTimeGetCurrent()
 }
}
let seconds=CFAbsoluteTimeGetCurrent()-start
var results=[[String:Any]]()
for i in 0..<n {
 let m=u32(metas,i*9,9),s=u32(states,i*stateWords,8)
 let raw=i64(rows,i*rowWords,Int(m[0])*stride),arena=i64(rows,i*rowWords+capacity*stride,program["set_words"] as! Int)
 let h=u32(history,i*historyCapacity*historyWidth,Int(s[3])*historyWidth)
 results.append(["row_index_rebuilds":i64(rows,i*rowWords+(program["index_counter_offset"] as! Int),1)[0],"meta":m,"state":s,"rows":Swift.stride(from:0,to:raw.count,by:stride).map{Array(raw[$0..<$0+stride])},
  "parents":u32(uints,i*uintWords+layout["parent"]!,Int(m[1])),"fact_parents":u32(uints,i*uintWords+layout["fp"]!,Int(m[8])),
  "batch_statistics":transportVersion==7 ? i64(batchData,i*batchWords,batchHeader):[],"region_statistics":cacheHeader>0 ? i64(markerCache,i*cacheWords,16):[],"watermarks":u32(uints,i*uintWords+layout["watermarks"]!,program["rules"] as! Int),"sets":Array(arena.prefix(Int(arena[0]))),
  "history":Swift.stride(from:0,to:h.count,by:historyWidth).map{Array(h[$0..<$0+historyWidth])},"completed":m[4]==0 && s[2]==1])
}
let report:[String:Any]=["instances":results,"inference_seconds":seconds,"gpu_seconds":gpuSeconds,"shader_compile_seconds":compileSeconds,
 "dispatches":dispatches,"storage_bytes":[rows,uints,longs,metas,states,code,tape,history,markerCache,batchData,hashData].reduce(0){$0+$1.length},
 "stage_decisions_on_cpu":0,"batch_context_growths":batchGrowths,"batch_capacity":batchCapacity,"region_cache_growths":cacheGrowths,"scope":spec["contract"]!]
try JSONSerialization.data(withJSONObject:report,options:[.sortedKeys]).write(to:output,options:.atomic)
