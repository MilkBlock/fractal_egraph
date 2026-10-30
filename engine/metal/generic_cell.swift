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
 try require(["--resume","--checkpoint","--stop-after-runs","--capture"].contains(key) && at+1<CommandLine.arguments.count && arguments[key]==nil,"invalid or repeated worker option: \(key)")
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
var privateScheduleExecution=program["resident_schedules"] is [String:Any]
if stopAt != nil {privateScheduleExecution=false}
if let path=arguments["--resume"],program["resident_schedules"] is [String:Any] {
 let manifest=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:path).appendingPathComponent("manifest.json"))) as! [String:Any]
 privateScheduleExecution=manifest["private_schedule_execution"] as? Bool ?? false
}
var semanticProgram=program;semanticProgram.removeValue(forKey:"quantum")
if program["resident_schedules"] is [String:Any] {semanticProgram["resident_private_schedule_execution"]=privateScheduleExecution}
let programDigest=digest(try JSONSerialization.data(withJSONObject:semanticProgram,options:[.sortedKeys]))
let workerURL=URL(fileURLWithPath:CommandLine.arguments[0]).resolvingSymlinksInPath()
let workerImage=try Data(contentsOf:workerURL)
let workerSourceImage=try? Data(contentsOf:workerURL.deletingPathExtension().appendingPathExtension("swift"))
let workerDigest=digest(workerImage)
let instances=program["instances"] as! [[String:Any]],n=instances.count
let device=MTLCreateSystemDefaultDevice()!,queue=device.makeCommandQueue()!
let options=MTLCompileOptions();options.fastMathEnabled=false
let compileStart=CFAbsoluteTimeGetCurrent()
FileHandle.standardError.write(Data("[metal] compiling library\n".utf8))
let lib=try device.makeLibrary(source:program["shader"] as! String,options:options)
let librarySeconds=CFAbsoluteTimeGetCurrent()-compileStart
FileHandle.standardError.write(Data("[metal] library ready in \(librarySeconds)s\n".utf8))
// Apple's process cache is insufficient for these large generated pipelines.
// A content-addressed binary archive makes an actual cache hit observable.
let cacheSetting=ProcessInfo.processInfo.environment["EGGFUSE_METAL_CACHE"]
let cacheRoot=URL(fileURLWithPath:cacheSetting ?? URL(fileURLWithPath:CommandLine.arguments[0]).deletingLastPathComponent().appendingPathComponent("metal-cache").path)
let cacheIdentity=(program["shader"] as! String)+"\nfast-math=false;pipeline-defaults-v1;"+String(device.registryID)+ProcessInfo.processInfo.operatingSystemVersionString+";capture="+(ProcessInfo.processInfo.environment["METAL_CAPTURE_ENABLED"] ?? "0")
let cacheKey=digest(Data(cacheIdentity.utf8)),cacheURL=cacheRoot.appendingPathComponent(digest(Data(cacheIdentity.utf8))+".metalarc")
var archive:MTLBinaryArchive?=nil,cacheHits=0,cacheMisses=0,cacheDirty=false,cacheWarnings=[String](),pipelineTimings=[String:Double]()
if cacheSetting != "off" {
 do {
  try FileManager.default.createDirectory(at:cacheRoot,withIntermediateDirectories:true)
  let descriptor=MTLBinaryArchiveDescriptor()
  if FileManager.default.fileExists(atPath:cacheURL.path){descriptor.url=cacheURL}
  do {archive=try device.makeBinaryArchive(descriptor:descriptor)}
  catch {cacheWarnings.append("archive load: \(error)");archive=try device.makeBinaryArchive(descriptor:MTLBinaryArchiveDescriptor())}
 }catch{cacheWarnings.append("archive unavailable: \(error)")}
}
func loadPipeline(_ name:String)throws->MTLComputePipelineState {
 let began=CFAbsoluteTimeGetCurrent();defer{pipelineTimings[name]=CFAbsoluteTimeGetCurrent()-began}
 let descriptor=MTLComputePipelineDescriptor();descriptor.computeFunction=lib.makeFunction(name:name)!
 if let saved=archive {
  descriptor.binaryArchives=[saved]
  if let state=try? device.makeComputePipelineState(descriptor:descriptor,options:.failOnBinaryArchiveMiss,reflection:nil){cacheHits+=1;return state}
  cacheMisses+=1
  FileHandle.standardError.write(Data("[metal] compiling pipeline \(name)\n".utf8))
  do {
   try saved.addComputePipelineFunctions(descriptor:descriptor);cacheDirty=true
   return try device.makeComputePipelineState(descriptor:descriptor,options:.failOnBinaryArchiveMiss,reflection:nil)
  }catch{cacheWarnings.append("archive population \(name): \(error)")}
 }else{cacheMisses+=1}
 // Cache failure never substitutes a different shader or a guessed result.
 descriptor.binaryArchives=[]
 return try device.makeComputePipelineState(descriptor:descriptor,options:[],reflection:nil)
}
let pipeline=try loadPipeline(spec["kernel"] as? String ?? "generic_cell_advance")
let batchEval:MTLComputePipelineState?=transportVersion==7 ? try loadPipeline("batched_cell_evaluate"):nil
let batchPublish:MTLComputePipelineState?=transportVersion==7 ? try loadPipeline("batched_cell_publish"):nil
let batchMatch:MTLComputePipelineState?=transportVersion==7 ? try loadPipeline("batched_cell_match"):nil
let batchPack:MTLComputePipelineState?=transportVersion==7 ? try loadPipeline("batched_cell_pack"):nil
let parallelSort=transportVersion==7 && (program["batch_sort"] as? [String:Any])?["enabled"] as? Bool == true
var sortSpans=[Int](),sortSpan=256
while sortSpan<(program["match_capacity"] as! Int){sortSpans.append(sortSpan);sortSpan*=2}
let sortLocal:MTLComputePipelineState?=parallelSort ? try loadPipeline("batched_cell_sort_local"):nil
let sortMerge:MTLComputePipelineState?=parallelSort ? try loadPipeline("batched_cell_sort_merge"):nil
let indexSpec=program["resident_indexes"] as? [String:Any]
let parallelIndexes=transportVersion==7 && indexSpec?["enabled"] as? Bool == true
let indexBase=indexSpec?["dispatch_base"] as? Int ?? 0
let indexFields=indexSpec?["dispatch_fields"] as? Int ?? 6
let hierarchicalIndexes=parallelIndexes && indexSpec?["hierarchical"] as? Bool == true
let indexNames=["query_row_scan","query_row_offsets","query_pairs","query_radix_scan","query_radix_offsets","query_radix_scatter","query_clear","query_link_sorted","query_table_time"]
let indexPipelines: [MTLComputePipelineState]=parallelIndexes ? try indexNames.map{try loadPipeline("resident_"+$0)}:[]
let radixBlocks:MTLComputePipelineState?=hierarchicalIndexes ? try loadPipeline("resident_query_radix_blocks"):nil
let hashIndexNames=["row_index_context","row_index_clear","row_index_parallel","row_index_ready"]
let hashIndexPipelines: [MTLComputePipelineState]=parallelIndexes ? try hashIndexNames.map{try loadPipeline("resident_"+$0)}:[]
let prepareFinish:MTLComputePipelineState?=parallelIndexes ? try loadPipeline("batched_cell_prepare_finish"):nil
let rebuildSpec=program["resident_rebuild"] as? [String:Any]
let parallelRebuild=transportVersion==7 && rebuildSpec?["enabled"] as? Bool == true
let rebuildBase=rebuildSpec?["dispatch_base"] as? Int ?? 0
let rebuildNames=["rebuild_gate","rebuild_start","rebuild_canonical","rebuild_insert","rebuild_assign","merge_scan","merge_offsets","merge_scatter","rebuild_merge","compact_gate","compact_scan","compact_offsets","compact_copy","compact_store","rebuild_finish"]
let rebuildPipelines:[MTLComputePipelineState]=parallelRebuild ? try rebuildNames.map{try loadPipeline("resident_"+$0)}:[]
let resumeMergeIndex=parallelRebuild && rebuildSpec?["resume_merge_index"] as? Bool == true
let mergeIndexNames=["merge_index_gate","merge_row_index_context","merge_row_index_clear","merge_row_index_parallel","merge_row_index_ready","rebuild_resume"]
let mergeIndexPipelines:[MTLComputePipelineState]=resumeMergeIndex ? try mergeIndexNames.map{try loadPipeline("resident_"+$0)}:[]
let writesSpec=program["resident_writes"] as? [String:Any]
let parallelWrites=transportVersion==7 && writesSpec?["enabled"] as? Bool == true
let writesBase=writesSpec?["dispatch_base"] as? Int ?? 0
let writesNames=["write_gate","batch_output_scan","batch_output_offsets","batch_clear","batch_propose","batch_group","batch_scan","batch_reserve","batch_publish","batch_index","batch_ordered","write_finish"]
let writesPipelines:[MTLComputePipelineState]=parallelWrites ? try writesNames.map{try loadPipeline("resident_"+$0)}:[]
let waveSpec=program["cell_waves"] as? [String:Any]
let cellWaves=transportVersion==7 && waveSpec?["enabled"] as? Bool == true
let waveBase=waveSpec?["dispatch_base"] as? Int ?? 0
let waveNames=["gate","clear","propose","group","link","scan","reserve","publish","index","resolve","cert_offsets","cert_write","cert_hash","finish"]
let wavePipelines:[MTLComputePipelineState]=cellWaves ? try waveNames.map{try loadPipeline("cell_wave_"+$0)}:[]
let unionNames=["gate","apply","flatten","finish"]
let unionPipelines:[MTLComputePipelineState]=cellWaves ? try unionNames.map{try loadPipeline("cell_union_"+$0)}:[]
let seedsSpec=program["resident_seeds"] as? [String:Any]
let compactSeeds=transportVersion==7 && seedsSpec?["enabled"] as? Bool == true
let seedsBase=seedsSpec?["dispatch_base"] as? Int ?? 0
let seedsNames=["seed_scan","seed_offsets","seed_scatter"]
let seedsPipelines:[MTLComputePipelineState]=compactSeeds ? try seedsNames.map{try loadPipeline("resident_"+$0)}:[]
let hashPipeline:MTLComputePipelineState?=transportVersion==7 ? try loadPipeline("batched_cell_fingerprint"):nil
let hashFinish:MTLComputePipelineState?=transportVersion==7 ? try loadPipeline("batched_cell_fingerprint_finish"):nil
// RESIDENT_SCHEDULE_RUNTIME
var cacheWriteSeconds=0.0
if cacheDirty,let saved=archive {
 let began=CFAbsoluteTimeGetCurrent(),temp=cacheRoot.appendingPathComponent("."+cacheKey+"-"+UUID().uuidString)
 do {
  try saved.serialize(to:temp)
  // rename replaces only this content-addressed cache entry, atomically.
  try require(rename(temp.path,cacheURL.path)==0,"cannot publish pipeline archive")
 }catch{cacheWarnings.append("archive write: \(error)");try? FileManager.default.removeItem(at:temp)}
 cacheWriteSeconds=CFAbsoluteTimeGetCurrent()-began
}
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
let dispatchGroups=3+(parallelSort ? 1+sortSpans.count:0)+(parallelIndexes ? indexFields:0)+(parallelRebuild ? 5:0)+(parallelWrites ? 4:0)+(compactSeeds ? 2:0)+(cellWaves ? 6:0)
let batchDispatch=zeros(dispatchGroups*12)
for i in 0..<dispatchGroups {copy([UInt32(0),UInt32(n),UInt32(1)],batchDispatch,i*12)}
let hashChunks=(capacity*stride+2*ids+(program["set_words"] as! Int)+1023)/1024
let hashData=zeros(transportVersion==7 ? n*hashChunks*16:8)
var historyCapacity=16,history=zeros(n*16*historyWidth*4),dispatches=0,gpuSeconds=0.0
// Growth description is a string, so read numeric fields individually.
let cacheProgram=program["region_cache"] as? [String:Any]
let cacheHeader=cacheProgram?["header_words"] as? Int ?? 0
var cacheWords=cacheProgram?["initial_words"] as? Int ?? 1
var markerCache=zeros(n*cacheWords*8),cacheGrowths=0
if cacheHeader>0 {for i in 0..<n {copy([Int64(cacheHeader),Int64(cacheWords)],markerCache,i*cacheWords*8)}}
var compactCapacity=parallelRebuild ? 128:1,compactGrowths=0
var compactScratch=zeros(parallelRebuild ? n*compactCapacity*stride*8:8)
func writeWords(_ cap:Int)->Int {64+(cap+255)/256+5*cap}
let proposalStride=stride-2
var writeCapacity=128,writeGrowths=0
var writeUInts=zeros(parallelWrites ? n*writeWords(writeCapacity)*4:8),writeLongs=zeros(parallelWrites ? n*writeCapacity*proposalStride*8:8)
func waveWords(_ cap:Int)->Int {128+8*cap+3*((cap+255)/256)}
let waveStride=waveSpec?["packet_stride"] as? Int ?? 1
var waveUInts=zeros(cellWaves ? n*waveWords(batchCapacity)*4:8),wavePackets=zeros(cellWaves ? n*batchCapacity*waveStride*8:8)
func buffers()->[(String,MTLBuffer)] {[("rows",rows),("uints",uints),("longs",longs),("metas",metas),("states",states),("history",history),("marker-cache",markerCache),("batch-data",batchData),("dispatch",batchDispatch),("hashes",hashData)]+(parallelRebuild ? [("compact-scratch",compactScratch)]:[])+(parallelWrites ? [("write-uints",writeUInts),("write-longs",writeLongs)]:[])}
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
 if parallelRebuild {compactCapacity=sizes["compact_capacity"]!;compactGrowths=sizes["compact_growths"]!;compactScratch=zeros(n*compactCapacity*stride*8)}
 if parallelWrites {writeCapacity=sizes["write_capacity"]!;writeGrowths=sizes["write_growths"]!;writeUInts=zeros(n*writeWords(writeCapacity)*4);writeLongs=zeros(n*writeCapacity*proposalStride*8)}
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
if cellWaves {waveUInts=zeros(n*waveWords(batchCapacity)*4);wavePackets=zeros(n*batchCapacity*waveStride*8)}
let capturing=arguments["--capture"] != nil
if let path=arguments["--capture"] {
 try require(n==1 && stopAt != nil && Int(stopAt!)==Int(restoredRounds[0])+1,"capture must execute exactly one Run from the loaded state")
 try require(!FileManager.default.fileExists(atPath:path),"capture destination already exists")
 for (name,b) in buffers(){b.label="resident."+name};code.label="resident.action-code";tape.label="resident.control"
 let descriptor=MTLCaptureDescriptor();descriptor.captureObject=queue
 descriptor.destination = .gpuTraceDocument;descriptor.outputURL=URL(fileURLWithPath:path)
 try require(MTLCaptureManager.shared().supportsDestination(.gpuTraceDocument),"GPU capture unavailable; set METAL_CAPTURE_ENABLED=1")
 try MTLCaptureManager.shared().startCapture(with:descriptor)
}
defer {if capturing && MTLCaptureManager.shared().isCapturing {MTLCaptureManager.shared().stopCapture()}}
let quantum=UInt32(program["quantum"] as? Int ?? 8)
let start=CFAbsoluteTimeGetCurrent()
var lastProgress=start
while true {
 // This is an explicit diagnostic suspension, not a saturation budget. The
 // GPU gate prevents a queued quantum from starting Run N+1 after N completes.
 if let stop=stopAt, (0..<n).allSatisfy({u32(states,$0*stateWords+2,1)[0]==1 || (u32(states,$0*stateWords+3,1)[0]>=stop && atBoundary($0))}) {break}
 var pending=false,needGrowth=false,needCacheGrowth=false,needBatchGrowth=false,needCompactGrowth=false,needWriteGrowth=false
 for i in 0..<n {
  let s=u32(states,i*stateWords,8),m=u32(metas,i*9,9)
  if s[2]==0 && (m[4]==0 || (cacheHeader>0 && (m[4]==31 || m[4]==37)) || (transportVersion==7 && m[4]==34) || (parallelRebuild && m[4]==38) || (parallelWrites && m[4]==40)) {
   pending=true;if s[4] != 0 {needGrowth=true};if m[4]==31 {needCacheGrowth=true};if m[4]==34 {needBatchGrowth=true};if m[4]==38 {needCompactGrowth=true};if m[4]==40 {needWriteGrowth=true}
  }
 }
 if !pending {break}
 if transportVersion==7 && n==1 && i64(batchData,10,1)[0]==8 {
  let before=u32(states,0,8),pc=Int(before[0]),sid=instructions[4*pc+1]
  hostMeta=u32(metas,0,9)
  let completed=try scheduledMatrix(sid)
  if completed.accepted {
   let count=completed.history.count/12,required=Int(before[3])+count
   while historyCapacity<required {
    let next=zeros(n*historyCapacity*2*historyWidth*4);memcpy(next.contents(),history.contents(),history.length);history=next;historyCapacity*=2
   }
   var trace=completed.history
   for i in 0..<count {
    trace[12*i+6]=UInt32(pc+1)+trace[12*i+5]
    trace[12*i+7]=before[1]+completed.history[12*i+6]
    trace[12*i+5]=hostMeta[7]
   }
   copy(trace,history,Int(before[3])*historyWidth*4)
   copy([instructions[4*pc+2]],states,0);copy([UInt32(required)],states,3*4)
   if before[1]>0 && completed.info[3] != 0 {copy([UInt32(1)],states,(8+8*(Int(before[1])-1)+5)*4)}
   if i64(batchData,14,1)[0] != 0 {copy([Int64(required)],batchData,22*8)}
   gpuSeconds+=completed.phases.reduce(0,+)
  }else{
   // Private failure published nothing. Retain the complete ordinary program.
   copy([before[0]+1],states,0)
  }
  copy([Int64(0)],batchData,10*8)
  continue
 }
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
   // Another arena may still have a block continuation. Its descriptors,
   // exact slots and done bits survive a neighbor's capacity growth.
   memcpy(next.contents().advanced(by:(i*newWords+batchHeader)*8),batchData.contents().advanced(by:(i*batchWords+batchHeader)*8),3*batchCapacity*8)
   memcpy(next.contents().advanced(by:(i*newWords+batchHeader+3*newCapacity)*8),batchData.contents().advanced(by:(i*batchWords+batchHeader+3*batchCapacity)*8),batchCapacity*batchStride*8)
   let oldRecords=batchHeader+batchCapacity*(5+batchStride),newRecords=batchHeader+newCapacity*(5+batchStride)
   memcpy(next.contents().advanced(by:(i*newWords+newRecords)*8),batchData.contents().advanced(by:(i*batchWords+oldRecords)*8),4*batchCapacity*8)
   // Retain the old-capacity marker until the GPU rehashes certificate buckets.
   if u32(metas,i*9+4,1)[0]==34 {copy([UInt32(0)],metas,(i*9+4)*4)}
  }
  batchData=next;batchWords=newWords;batchCapacity=newCapacity;batchGrowths+=1
  if cellWaves {waveUInts=zeros(n*waveWords(batchCapacity)*4);wavePackets=zeros(n*batchCapacity*waveStride*8)}
 }
 if needCompactGrowth {
  let required=(0..<n).map{Int(u32(metas,$0*9,1)[0])}.max() ?? 1
  while compactCapacity<required {compactCapacity*=2;compactGrowths+=1}
  compactScratch=zeros(n*compactCapacity*stride*8)
  for i in 0..<n {if u32(metas,i*9+4,1)[0]==38 {copy([UInt32(0)],metas,(i*9+4)*4)}}
 }
 if needWriteGrowth {
  let required=(0..<n).map{Int(u32(writeUInts,$0*writeWords(writeCapacity)+43,1)[0])}.max() ?? 1
  while writeCapacity<required {writeCapacity*=2;writeGrowths+=1}
  writeUInts=zeros(n*writeWords(writeCapacity)*4);writeLongs=zeros(n*writeCapacity*proposalStride*8)
  for i in 0..<n {if u32(metas,i*9+4,1)[0]==40 {copy([UInt32(0)],metas,(i*9+4)*4)}}
 }
 if needGrowth {
  let next=zeros(n*historyCapacity*2*historyWidth*4)
  for i in 0..<n {memcpy(next.contents().advanced(by:i*historyCapacity*2*historyWidth*4),history.contents().advanced(by:i*historyCapacity*historyWidth*4),historyCapacity*historyWidth*4);copy([UInt32(0)],states,(i*stateWords+4)*4)}
  history=next;historyCapacity*=2
 }
 let cb=queue.makeCommandBuffer()!
 cb.label=transportVersion==7 ? "resident run=\(u32(states,3,1)[0]+1) pc=\(u32(states,0,1)[0])":"generic Cell local schedule"
 func bind(_ e:MTLComputeCommandEncoder){
  let buffers=[rows,uints,longs,metas,states,code,tape,history]+(transportVersion>=2 ? [markerCache]:[])
  for (i,b) in buffers.enumerated(){e.setBuffer(b,offset:0,index:i)}
  var cfg=SIMD4<UInt32>(UInt32(n),quantum,UInt32(historyCapacity),UInt32(cacheWords))
  e.setBytes(&cfg,length:16,index:transportVersion>=2 ? 9:8)
  if transportVersion==7 {
   e.setBuffer(batchData,offset:0,index:10)
   var batchCfg=SIMD4<UInt32>(UInt32(batchCapacity),stopAt ?? 0,UInt32(compactCapacity),privateScheduleExecution ? 0:1);e.setBytes(&batchCfg,length:16,index:11)
   e.setBuffer(batchDispatch,offset:0,index:12);e.setBuffer(hashData,offset:0,index:13)
   if parallelRebuild {e.setBuffer(compactScratch,offset:0,index:15)}
   var writeCfg=SIMD4<UInt32>(UInt32(writeCapacity),0,0,0);e.setBytes(&writeCfg,length:16,index:16)
   e.setBuffer(writeUInts,offset:0,index:17);e.setBuffer(writeLongs,offset:0,index:18)
   if cellWaves {e.setBuffer(waveUInts,offset:0,index:19);e.setBuffer(wavePackets,offset:0,index:20)}
   var packStep=UInt32(0);e.setBytes(&packStep,length:4,index:14)
  }
 }
 if transportVersion==7 {
  try require(batchEval!.maxTotalThreadsPerThreadgroup>=128,"batch evaluator requires 128 lanes")
  if parallelSort {try require(sortLocal!.maxTotalThreadsPerThreadgroup>=256,"local ordering requires 256 lanes")}
  for _ in 0..<quantum {
   let clear=cb.makeBlitCommandEncoder()!
   for i in 0..<dispatchGroups {clear.fill(buffer:batchDispatch,range:(i*12)..<(i*12+4),value:0)}
   clear.endEncoding()
   var e=cb.makeComputeCommandEncoder()!;bind(e);var phaseStarted=false
   func phase(_ p:MTLComputePipelineState,_ name:String){
    // Capture-only separation makes phase attribution explicit. The normal
    // execution path retains its single encoder and existing barriers.
    if capturing && phaseStarted {e.endEncoding();e=cb.makeComputeCommandEncoder()!;bind(e)}
    if capturing {e.label="fn=\(name) resident"}
    e.setComputePipelineState(p);phaseStarted=true
   }
   phase(pipeline,"batched_cell_prepare");e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(32,pipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   if parallelIndexes {
    for (index,field) in [3,5,4,3].enumerated(){
     phase(hashIndexPipelines[index],"resident_"+hashIndexNames[index])
     e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12*(indexBase+field),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))
     e.memoryBarrier(scope:.buffers)
    }
    func indexPhase(_ index:Int,_ field:Int,_ bit:UInt32=0){
     phase(indexPipelines[index],"resident_"+indexNames[index])
     // phase() may open a fresh encoder during capture. Bind its radix pass
     // afterwards so capture and ordinary execution use identical parameters.
     var radixBit=bit;e.setBytes(&radixBit,length:4,index:14)
     e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12*(indexBase+field),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))
     e.memoryBarrier(scope:.buffers)
    }
    indexPhase(0,0);indexPhase(1,3);indexPhase(2,0)
    for bit in 0..<(indexSpec!["radix_passes"] as! Int){
     indexPhase(3,1,UInt32(bit))
     if hierarchicalIndexes {
      phase(radixBlocks!,"resident_query_radix_blocks")
      e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12*(indexBase+6),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));e.memoryBarrier(scope:.buffers)
     }
     indexPhase(4,3,UInt32(bit));indexPhase(5,1,UInt32(bit))
    }
    indexPhase(6,2);indexPhase(7,1);indexPhase(8,0)
    phase(prepareFinish!,"batched_cell_prepare_finish")
    e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(32,prepareFinish!.maxTotalThreadsPerThreadgroup),height:1,depth:1))
    e.memoryBarrier(scope:.buffers)
   }
   if compactSeeds {
    for (index,field) in [0,1,0].enumerated(){
     phase(seedsPipelines[index],"resident_"+seedsNames[index])
     e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12*(seedsBase+field),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))
     e.memoryBarrier(scope:.buffers)
    }
   }
   phase(batchMatch!,"batched_cell_match");e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:24,threadsPerThreadgroup:MTLSize(width:128,height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   if parallelSort {
    phase(batchPack!,"batched_cell_pack_or_schedule");var firstPack=UInt32(0);e.setBytes(&firstPack,length:4,index:14)
    e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))
    e.memoryBarrier(scope:.buffers)
    phase(sortLocal!,"batched_cell_sort_local");e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:36,threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))
    e.memoryBarrier(scope:.buffers)
    // Ordinary Runs do not grow the Cell frame buffer. Sorting is sized by
    // the independent match arena; its GPU dispatch already gates unused passes.
    for (pass,span) in sortSpans.enumerated() {
     phase(sortMerge!,"batched_cell_sort_merge")
     let cap=UInt32(program["match_capacity"] as! Int)
     var sortCfg=SIMD4<UInt32>(UInt32(span),pass%2==0 ? 0:cap,pass%2==0 ? cap:0,0)
     e.setBytes(&sortCfg,length:16,index:14)
     e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:48+12*pass,threadsPerThreadgroup:MTLSize(width:128,height:1,depth:1))
     e.memoryBarrier(scope:.buffers)
    }
   }
   phase(batchPack!,"batched_cell_pack");var finalPack=UInt32(parallelSort ? 1:0);e.setBytes(&finalPack,length:4,index:14)
   e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:parallelSort ? 256:1,height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   phase(batchEval!,"batched_cell_evaluate");e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:0,threadsPerThreadgroup:MTLSize(width:128,height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   phase(batchPublish!,"batched_cell_publish");e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(32,batchPublish!.maxTotalThreadsPerThreadgroup),height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   if cellWaves {
    for (i,field) in [2,1,0,0,0,0,2,0,0,0,2,3,3,2].enumerated(){
     phase(wavePipelines[i],"cell_wave_"+waveNames[i])
     if i==0 {e.dispatchThreadgroups(MTLSize(width:1,height:n,depth:1),threadsPerThreadgroup:MTLSize(width:1,height:1,depth:1))}
     else {e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12*(waveBase+field),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))}
     e.memoryBarrier(scope:.buffers)
    }
   }
   if cellWaves {
    for (i,field) in [2,4,5,2].enumerated(){
     phase(unionPipelines[i],"cell_union_"+unionNames[i])
     if i==0 {e.dispatchThreadgroups(MTLSize(width:1,height:n,depth:1),threadsPerThreadgroup:MTLSize(width:1,height:1,depth:1))}
     else {e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12*(waveBase+field),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))}
     e.memoryBarrier(scope:.buffers)
    }
   }
   if parallelWrites {
    func writesPhase(_ index:Int,_ field:Int?=nil){
     phase(writesPipelines[index],"resident_"+writesNames[index])
     if let field=field {e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12*(writesBase+field),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))}
     else {e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:1,height:1,depth:1))}
     e.memoryBarrier(scope:.buffers)
    }
    writesPhase(0);writesPhase(1,0);writesPhase(2,3);writesPhase(3,2);writesPhase(4,0)
    writesPhase(5,1);writesPhase(6,1);writesPhase(7,3);writesPhase(8,1);writesPhase(9,1);writesPhase(10,3);writesPhase(11,3)
   }
   if parallelRebuild {
    func rebuildPhase(_ index:Int,_ field:Int?=nil){
     phase(rebuildPipelines[index],"resident_"+rebuildNames[index])
     if let field=field {
      e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12*(rebuildBase+field),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))
     }else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:1,height:1,depth:1))}
     e.memoryBarrier(scope:.buffers)
    }
    rebuildPhase(0);rebuildPhase(1,2);rebuildPhase(2,0);rebuildPhase(3,1);rebuildPhase(4,1)
    rebuildPhase(5,1);rebuildPhase(6,2);rebuildPhase(7,1);rebuildPhase(8,2)
    if resumeMergeIndex {
     for (i,field) in [2,2,0,1,2,2].enumerated(){
      phase(mergeIndexPipelines[i],"resident_"+mergeIndexNames[i])
      if i==0 {e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:1,height:1,depth:1))}
      else {e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12*(rebuildBase+field),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))}
      e.memoryBarrier(scope:.buffers)
     }
    }
    rebuildPhase(9);rebuildPhase(10,3);rebuildPhase(11,4);rebuildPhase(12,3);rebuildPhase(13,3);rebuildPhase(14,4)
   }
   phase(hashPipeline!,"batched_cell_fingerprint");e.dispatchThreadgroups(indirectBuffer:batchDispatch,indirectBufferOffset:12,threadsPerThreadgroup:MTLSize(width:128,height:1,depth:1))
   e.memoryBarrier(scope:.buffers)
   phase(hashFinish!,"batched_cell_fingerprint_finish");e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(32,hashFinish!.maxTotalThreadsPerThreadgroup),height:1,depth:1))
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
if capturing {MTLCaptureManager.shared().stopCapture()}
var checkpointSeconds=0.0
if let url=checkpointURL {
 for i in 0..<n {try require(atBoundary(i),"refusing checkpoint with unfinished Run or execution error")}
 let began=CFAbsoluteTimeGetCurrent(),temporary=url.deletingLastPathComponent().appendingPathComponent("."+url.lastPathComponent+"-"+UUID().uuidString)
 try FileManager.default.createDirectory(at:temporary,withIntermediateDirectories:true)
 var files=[String:[String:Any]]()
 for (name,b) in buffers(){files[name]=["bytes":b.length,"sha256":try writeBuffer(b,temporary.appendingPathComponent(name+".bin"))]}
 try workerImage.write(to:temporary.appendingPathComponent("worker"),options:.atomic)
 try? FileManager.default.setAttributes([.posixPermissions:0o755],ofItemAtPath:temporary.appendingPathComponent("worker").path)
 var frozenWorker:[String:Any]=["binary":"worker","sha256":digest(workerImage)]
 if let source=workerSourceImage {try source.write(to:temporary.appendingPathComponent("worker.swift"),options:.atomic);frozenWorker["source"]="worker.swift";frozenWorker["source_sha256"]=digest(source)}
 let manifest:[String:Any]=["frozen_worker":frozenWorker,"schema":"eggfuse-resident-checkpoint/v1","program_sha256":programDigest,"worker_sha256":workerDigest,"device":device.name,"instances":n,"buffers":files,
  "sizes":["batch_capacity":batchCapacity,"history_capacity":historyCapacity,"cache_words":cacheWords,"batch_growths":batchGrowths,"cache_growths":cacheGrowths,"compact_capacity":compactCapacity,"compact_growths":compactGrowths,"write_capacity":writeCapacity,"write_growths":writeGrowths],
  "completed_runs":(0..<n).map{u32(states,$0*stateWords+3,1)[0]},"program_path":input.path,
  "private_schedule_execution":privateScheduleExecution,"contract":"full resident buffers at completed command buffer and logical Run boundary; no actions replayed; exact-Run diagnostics pin ordinary schedule execution"]
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
 "library_compile_seconds":librarySeconds,"pipeline_compile_seconds":pipelineTimings,
 "pipeline_cache":["key":cacheKey,"path":cacheURL.path,"hits":cacheHits,"misses":cacheMisses,"write_seconds":cacheWriteSeconds,"warnings":cacheWarnings,"disabled":cacheSetting=="off"],
 "paused":results.contains{!($0["completed"] as! Bool)} && stopAt != nil,"restored_runs":restoredRounds,"checkpoint_seconds":checkpointSeconds,"program_sha256":programDigest,"worker_sha256":workerDigest,
 "profile_capture":capturing,
 "dispatches":dispatches,"storage_bytes":[rows,uints,longs,metas,states,code,tape,history,markerCache,batchData,hashData,compactScratch,writeUInts,writeLongs].reduce(0){$0+$1.length},
 "stage_decisions_on_cpu":0,"private_schedule_execution":privateScheduleExecution,"private_schedule_regions":smCalls,"private_schedule_runs":smRuns,"private_schedule_binding_outputs":smBindingTotal,"private_schedule_rows":smRows,"private_schedule_forwarded_reads":smForwardedTotal,"private_schedule_peak_bytes":msPeakBytes,"batch_context_growths":batchGrowths,"batch_capacity":batchCapacity,"region_cache_growths":cacheGrowths,"compact_capacity":compactCapacity,"compact_growths":compactGrowths,"write_capacity":writeCapacity,"write_growths":writeGrowths,"scope":spec["contract"]!]
try JSONSerialization.data(withJSONObject:report,options:[.sortedKeys]).write(to:output,options:.atomic)
