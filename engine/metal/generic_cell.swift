// Batch transport for independent typed Cell programs. CPU never executes a
// stage, joins bindings, computes a guard, or supplies an inferred endpoint.
import Foundation
import Metal
import CryptoKit

func require(_ ok:Bool,_ message:String)throws {if !ok {throw NSError(domain:message,code:1)}}
try require(CommandLine.arguments.count>=3,"usage: generic-cell-worker program.json result.json [--resume DIR] [--checkpoint DIR] [--stop-after-runs N]")
var arguments=[String:String](),at=3
while at<CommandLine.arguments.count {
 let key=CommandLine.arguments[at]
 try require(["--resume","--checkpoint","--stop-after-runs"].contains(key) && at+1<CommandLine.arguments.count && arguments[key]==nil,"invalid or repeated worker option: \(key)")
 arguments[key]=CommandLine.arguments[at+1];at+=2
}
var stopAt:UInt32?=nil
if let value=arguments["--stop-after-runs"] {try require(UInt32(value) != nil,"stop-after-runs must be an absolute nonnegative Run count");stopAt=UInt32(value)}
try require(stopAt==nil || arguments["--checkpoint"] != nil,"a suspended execution must have a checkpoint destination")
let checkpointURL=arguments["--checkpoint"].map{URL(fileURLWithPath:$0)}
if let url=checkpointURL {try require(!FileManager.default.fileExists(atPath:url.path),"checkpoint destination already exists")}
let input=URL(fileURLWithPath:CommandLine.arguments[1]), output=URL(fileURLWithPath:CommandLine.arguments[2])
let program=try JSONSerialization.jsonObject(with:Data(contentsOf:input)) as! [String:Any]
let spec=program["generic_cell"] as! [String:Any],layout=spec["layout"] as! [String:Int]
let transportVersion=spec["transport_version"] as? Int ?? 1
try require(transportVersion==1 || transportVersion==2 || transportVersion==7,"unknown local worker transport version")
if !arguments.isEmpty {try require(transportVersion==7 && spec["checkpoint_version"] as? Int == 1,"checkpoint requires a freshly compiled resident ABI7 program")}
func digest(_ data:Data)->String {SHA256.hash(data:data).map{String(format:"%02x",$0)}.joined()}
// Dispatch quantum is a host batching choice, never part of logical state.
var semanticProgram=program;semanticProgram.removeValue(forKey:"quantum")
let programDigest=digest(try JSONSerialization.data(withJSONObject:semanticProgram,options:[.sortedKeys]))
let workerDigest=digest(try Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[0]).resolvingSymlinksInPath()))
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
func buffers()->[(String,MTLBuffer)] {[("rows",rows),("uints",uints),("longs",longs),("metas",metas),("states",states),("history",history),("marker-cache",markerCache),("batch-data",batchData),("dispatch",batchDispatch),("hashes",hashData)]}
let chunkBytes=8*1024*1024
func writeBuffer(_ b:MTLBuffer,_ url:URL)throws->String {
 try require(FileManager.default.createFile(atPath:url.path,contents:nil),"cannot create checkpoint buffer")
 let f=try FileHandle(forWritingTo:url);defer{try? f.close()};var hash=SHA256()
 for start in Swift.stride(from:0,to:b.length,by:chunkBytes){
  let data=Data(bytesNoCopy:b.contents().advanced(by:start),count:min(chunkBytes,b.length-start),deallocator:.none)
  hash.update(data:data);try f.write(contentsOf:data)
 }
 try f.synchronize();return hash.finalize().map{String(format:"%02x",$0)}.joined()
}
func restoreBuffer(_ b:MTLBuffer,_ url:URL,_ expected:String)throws {
 let size=try FileManager.default.attributesOfItem(atPath:url.path)[.size] as! NSNumber
 try require(size.intValue==b.length,"checkpoint buffer size differs: \(url.lastPathComponent)")
 let f=try FileHandle(forReadingFrom:url);defer{try? f.close()};var hash=SHA256(),offset=0
 while offset<b.length {
  let data=try f.read(upToCount:min(chunkBytes,b.length-offset)) ?? Data()
  try require(!data.isEmpty,"truncated checkpoint");hash.update(data:data)
  _ = data.withUnsafeBytes{memcpy(b.contents().advanced(by:offset),$0.baseAddress!,data.count)};offset+=data.count
 }
 try require(hash.finalize().map{String(format:"%02x",$0)}.joined()==expected,"checkpoint checksum mismatch: \(url.lastPathComponent)")
}
var restoredRounds=[UInt32](repeating:0,count:n)
if let path=arguments["--resume"] {
 let dir=URL(fileURLWithPath:path),manifest=try JSONSerialization.jsonObject(with:Data(contentsOf:dir.appendingPathComponent("manifest.json"))) as! [String:Any]
 try require(manifest["schema"] as? String=="eggfuse-resident-checkpoint/v1" && manifest["program_sha256"] as? String==programDigest && manifest["worker_sha256"] as? String==workerDigest,"checkpoint program/worker contract differs")
 try require(manifest["device"] as? String==device.name && manifest["instances"] as? Int==n,"checkpoint device or instance count differs")
 let sizes=manifest["sizes"] as! [String:Int]
 batchCapacity=sizes["batch_capacity"]!;batchWords=batchHeader+batchCapacity*(9+batchStride);batchData=zeros(n*batchWords*8)
 historyCapacity=sizes["history_capacity"]!;history=zeros(n*historyCapacity*historyWidth*4)
 cacheWords=sizes["cache_words"]!;markerCache=zeros(n*cacheWords*8)
 batchGrowths=sizes["batch_growths"]!;cacheGrowths=sizes["cache_growths"]!
 let files=manifest["buffers"] as! [String:[String:Any]]
 try require(Set(files.keys)==Set(buffers().map{$0.0}),"checkpoint buffer inventory differs")
 for (name,b) in buffers(){try require(files[name]!["bytes"] as? Int==b.length,"checkpoint layout differs");try restoreBuffer(b,dir.appendingPathComponent(name+".bin"),files[name]!["sha256"] as! String)}
 restoredRounds=(0..<n).map{u32(states,$0*stateWords+3,1)[0]}
}
func atBoundary(_ i:Int)->Bool {
 let s=u32(states,i*stateWords,8),m=u32(metas,i*9,9)
 return m[4]==0 && s[7]==0 && m[6]==s[3] && i64(batchData,i*batchWords+10,1)[0]==0 && (i64(batchData,i*batchWords+14,1)[0]==0 || i64(batchData,i*batchWords+22,1)[0]==Int64(s[3]))
}
for i in 0..<n {if arguments["--resume"] != nil {try require(atBoundary(i),"checkpoint is not a completed Run boundary")}}
if let stop=stopAt {try require(restoredRounds.allSatisfy{$0<=stop},"stop-after-runs precedes checkpoint")}
let quantum=UInt32(program["quantum"] as? Int ?? 8)
let start=CFAbsoluteTimeGetCurrent()
var lastProgress=start
while true {
 // This is an explicit diagnostic suspension, not a saturation budget. The
 // GPU gate prevents a queued quantum from starting Run N+1 after N completes.
 if let stop=stopAt, (0..<n).allSatisfy({u32(states,$0*stateWords+2,1)[0]==1 || (u32(states,$0*stateWords+3,1)[0]>=stop && atBoundary($0))}) {break}
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
   var batchCfg=SIMD4<UInt32>(UInt32(batchCapacity),stopAt ?? 0,0,0);e.setBytes(&batchCfg,length:16,index:11)
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
var checkpointSeconds=0.0
if let url=checkpointURL {
 for i in 0..<n {try require(atBoundary(i),"refusing checkpoint with unfinished Run or execution error")}
 let began=CFAbsoluteTimeGetCurrent(),temporary=url.deletingLastPathComponent().appendingPathComponent("."+url.lastPathComponent+"-"+UUID().uuidString)
 try FileManager.default.createDirectory(at:temporary,withIntermediateDirectories:true)
 var files=[String:[String:Any]]()
 for (name,b) in buffers(){files[name]=["bytes":b.length,"sha256":try writeBuffer(b,temporary.appendingPathComponent(name+".bin"))]}
 let manifest:[String:Any]=["schema":"eggfuse-resident-checkpoint/v1","program_sha256":programDigest,"worker_sha256":workerDigest,"device":device.name,"instances":n,"buffers":files,
  "sizes":["batch_capacity":batchCapacity,"history_capacity":historyCapacity,"cache_words":cacheWords,"batch_growths":batchGrowths,"cache_growths":cacheGrowths],
  "completed_runs":(0..<n).map{u32(states,$0*stateWords+3,1)[0]},"program_path":input.path,
  "contract":"full resident buffers at completed command buffer and logical Run boundary; no actions replayed"]
 try JSONSerialization.data(withJSONObject:manifest,options:[.sortedKeys,.prettyPrinted]).write(to:temporary.appendingPathComponent("manifest.json"),options:.atomic)
 try FileManager.default.moveItem(at:temporary,to:url);checkpointSeconds=CFAbsoluteTimeGetCurrent()-began
}
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
 "paused":results.contains{!($0["completed"] as! Bool)} && stopAt != nil,"restored_runs":restoredRounds,"checkpoint_seconds":checkpointSeconds,"program_sha256":programDigest,"worker_sha256":workerDigest,
 "dispatches":dispatches,"storage_bytes":[rows,uints,longs,metas,states,code,tape,history,markerCache,batchData,hashData].reduce(0){$0+$1.length},
 "stage_decisions_on_cpu":0,"batch_context_growths":batchGrowths,"batch_capacity":batchCapacity,"region_cache_growths":cacheGrowths,"scope":spec["contract"]!]
try JSONSerialization.data(withJSONObject:report,options:[.sortedKeys]).write(to:output,options:.atomic)
