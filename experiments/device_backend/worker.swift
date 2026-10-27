import Foundation
import Metal
func need(_ ok:Bool,_ message:String)throws{if !ok{throw NSError(domain:message,code:1)}}
let a=CommandLine.arguments
try need(a.count==2,"worker program.json")
var program=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:a[1]))) as! [String:Any]
try need(program["schema"] as? String == "egglog-device-storage/v1","schema")
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue() else{fatalError("Metal unavailable")}
let opts=MTLCompileOptions();opts.fastMathEnabled=false
let lib=try device.makeLibrary(source:program["shader"] as! String,options:opts)
let initPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"initialize")!)
let keyFlagPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"initialize_key_flags")!)
let preparePipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"prepare")!)
let prepareClearPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"prepare_clear")!)
let prepareBuildPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"prepare_build")!)
let commitPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"commit_effects")!)
let unionPipelines=try ["commit_union_start","commit_union_parallel","commit_union_finish"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let noopPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"check_empty_effects")!)
let effectPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"group_same_effects")!)
let rebuildPipelines=try ["rebuild_start","rebuild_canonical","rebuild_insert","rebuild_assign","merge_scan","merge_offsets","merge_scatter","rebuild_merge","compact_scan","compact_offsets","compact_copy","compact_store"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let rebuildDispatch=try device.makeComputePipelineState(function:lib.makeFunction(name:"rebuild_dispatch")!)
let rebuildBatch=program["rebuild_batch"] as? Int ?? 1
try need(rebuildBatch>0,"rebuild batch must be positive")
try need(rebuildPipelines[4].maxTotalThreadsPerThreadgroup>=256 && rebuildPipelines[8].maxTotalThreadsPerThreadgroup>=256,"rebuild scans require 256-thread groups")
let rowIndexPipelines=try ["row_index_context","row_index_clear","row_index_parallel","row_index_ready"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let queryPipelines=try ["query_row_scan","query_row_offsets","query_pairs","query_radix_scan","query_radix_offsets","query_radix_scatter","query_link_sorted","query_table_time"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
try need(queryPipelines[0].maxTotalThreadsPerThreadgroup>=256 && queryPipelines[3].maxTotalThreadsPerThreadgroup>=256,"index scans require 256-thread groups")
let matchSortPipelines=try ["match_order_local","match_order_global","match_order_merge"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
try need(matchSortPipelines[0].maxTotalThreadsPerThreadgroup>=256 && matchSortPipelines[2].maxTotalThreadsPerThreadgroup>=256,"matching order requires 256-thread groups")
let stepPipelines=try (program["kernel_names"] as! [String]).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
func optionalPipelines(_ name:String)throws->[MTLComputePipelineState?]{
 try (program[name] as! [Any]).map{value in guard let s=value as? String else{return nil};return try device.makeComputePipelineState(function:lib.makeFunction(name:s)!)}
}
let prefixPipelines=try optionalPipelines("prefix_kernel_names"),fallbackPipelines=try optionalPipelines("fallback_kernel_names")
let contextPipelines=try ["context_reset","context_dispatch","context_resume","context_restart"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let contextPredictPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"context_predict")!)
let contextExportPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"context_training_export")!)
let contextRidge=(program["context_ridge"] as! [String:Any])["mode"] as! String == "ridge"
let contextTraining=program["context_training"] as? Bool ?? false
let batchDescription=program["batch_rhs"] as? [String:Any]
let batchEnabled=batchDescription?["enabled"] as? Bool ?? false
let batchNames=["batch_begin","batch_open","batch_clear","batch_propose","batch_group","batch_scan","batch_reserve","batch_publish","batch_index","batch_ordered","batch_finish","batch_active_scan","batch_active_offsets","batch_active_scatter","batch_output_scan","batch_output_offsets"]
let batchPipelines=try (batchEnabled ? batchNames:[]).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let batchSpec=batchDescription?["segments"] as? [[Int]] ?? []
let batchSegments=batchDescription?["set_segments"] as? [[Int]] ?? []
let capacity=program["capacity"] as! Int,stride=program["stride"] as! Int,ids=program["id_capacity"] as! Int
let matchCapacity=program["match_capacity"] as! Int,matchStride=program["match_stride"] as! Int,nrules=program["rules"] as! Int
func privateBuffer(_ bytes:Int)->MTLBuffer{device.makeBuffer(length:max(8,bytes),options:.storageModePrivate)!}
func shared<T>(_ x:[T])->MTLBuffer{x.withUnsafeBytes{b in b.isEmpty ? device.makeBuffer(length:8,options:.storageModeShared)! : device.makeBuffer(bytes:b.baseAddress!,length:b.count,options:.storageModeShared)!}}
let setWords=program["set_words"] as! Int
var rows=privateBuffer((capacity*stride+setWords+(program["index_words"] as! Int))*8),parent=privateBuffer((ids+(program["union_words"] as! Int))*4),factParent=privateBuffer(ids*4)
var old=rows
var meta=privateBuffer(36),matches=privateBuffer((program["workspace_words"] as! Int)*8),watermarks=privateBuffer(nrules*4)
let instructions=privateBuffer(max(8,(program["action_code"] as! [NSNumber]).count*8)),registers=privateBuffer((program["register_count"] as! Int)*8)
let queryHeads=privateBuffer((program["query_head_words"] as! Int)*4),queryLinks=privateBuffer((program["query_link_words"] as! Int)*4)
let queryPrevious=privateBuffer((program["query_prev_words"] as! Int)*4)
let rebuildControl=privateBuffer(160)
let queryAlternate=privateBuffer((program["query_sort_words"] as! Int)*8)
let matchOrder=privateBuffer((program["match_order_capacity"] as! Int)*4)
let noopFlags=privateBuffer((matchCapacity+1)*4)
let effectHeads=privateBuffer(2*(program["match_order_capacity"] as! Int)*4)
let contexts=privateBuffer((program["context_capacity"] as! Int)*(program["context_stride"] as! Int)*8),contextInfo=privateBuffer(64)
let contextResults=privateBuffer((program["context_capacity"] as! Int)*8)
let batchState=privateBuffer(batchEnabled ? (batchDescription!["state_words"] as! Int)*4:8)
let batchRows=privateBuffer(batchEnabled ? (batchDescription!["proposal_words"] as! Int)*8:8)
let batchStatus=device.makeBuffer(length:64,options:.storageModeShared)!
var batchTotals=[UInt64](repeating:0,count:4)
let initialMeta=(program["meta"] as! [NSNumber]).map{$0.uint32Value}
func initializeState() throws {
let initial=(program["initial_rows"] as! [NSNumber]).map{$0.int64Value}
let initCmd=queue.makeCommandBuffer()!,blit=initCmd.makeBlitCommandEncoder()!
for b in [rows,parent,factParent,meta,matches,watermarks,instructions,registers,queryHeads,rebuildControl]{blit.fill(buffer:b,range:0..<b.length,value:0)}
let actionStage=shared((program["action_code"] as! [NSNumber]).map{$0.int64Value})
blit.copy(from:actionStage,sourceOffset:0,to:instructions,destinationOffset:0,size:actionStage.length)
let staging=shared(initial),stagingMeta=shared(initialMeta),stagingSets=shared((program["initial_sets"] as! [NSNumber]).map{$0.int64Value})
blit.copy(from:stagingSets,sourceOffset:0,to:rows,destinationOffset:capacity*stride*8,size:stagingSets.length)
if !initial.isEmpty{blit.copy(from:staging,sourceOffset:0,to:rows,destinationOffset:0,size:initial.count*8)}
blit.copy(from:stagingMeta,sourceOffset:0,to:meta,destinationOffset:0,size:36);blit.endEncoding()
let encoder=initCmd.makeComputeCommandEncoder()!;encoder.setComputePipelineState(initPipeline);encoder.setBuffer(parent,offset:0,index:0);encoder.setBuffer(factParent,offset:0,index:1)
encoder.dispatchThreads(MTLSize(width:ids,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,initPipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1));encoder.endEncoding();initCmd.commit();initCmd.waitUntilCompleted()
try need(initCmd.status == .completed,"initialize failed")
let restore=queue.makeCommandBuffer()!,rb=restore.makeBlitCommandEncoder()!
let dirty=shared([Int64(1)]);rb.copy(from:dirty,sourceOffset:0,to:rows,destinationOffset:(program["query_dirty_offset"] as! Int)*8,size:8)
var stages:[MTLBuffer]=[dirty]
for (name,target) in [("initial_parents",parent),("initial_fact_parents",factParent),("initial_watermarks",watermarks)]{
 if let values=program[name] as? [NSNumber]{let b=shared(values.map{$0.uint32Value});stages.append(b);rb.copy(from:b,sourceOffset:0,to:target,destinationOffset:0,size:values.count*4)}
}
rb.endEncoding()
let flags=restore.makeComputeCommandEncoder()!;flags.setComputePipelineState(keyFlagPipeline)
flags.setBuffer(rows,offset:0,index:0);flags.setBuffer(parent,offset:0,index:2);flags.setBuffer(meta,offset:0,index:4)
flags.dispatchThreads(MTLSize(width:max(1,Int(initialMeta[0])),height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));flags.endEncoding()
restore.commit();restore.waitUntilCompleted();try need(restore.status == .completed,"restore failed")
withExtendedLifetime(stages) {}
}
try initializeState()
program.removeValue(forKey:"initial_rows");program.removeValue(forKey:"initial_sets");program.removeValue(forKey:"action_code");for n in ["initial_parents","initial_fact_parents","initial_watermarks"]{program.removeValue(forKey:n)}
func encode(_ cb:MTLCommandBuffer,_ pipeline:MTLComputePipelineState,_ threads:Int,_ arg:UInt32,indirect:Bool=false,indirectOffset:Int=16,radixBit:UInt32=0,sortK:UInt32=0,sortD:UInt32=0,effectMask:UInt32=1,contextPhase:UInt32=1,contextExport:MTLBuffer?=nil,sharedEncoder:MTLComputeCommandEncoder?=nil,bindResources:Bool=true,rebuildMode:UInt32=0,batchIndirectOffset:Int?=nil){
 let e=sharedEncoder ?? cb.makeComputeCommandEncoder()!;e.setComputePipelineState(pipeline)
 if bindResources {
 for (i,b) in [rows,old,parent,factParent,meta,matches,watermarks].enumerated(){e.setBuffer(b,offset:0,index:i)}
 e.setBuffer(instructions,offset:0,index:8);e.setBuffer(registers,offset:0,index:9);e.setBuffer(queryHeads,offset:0,index:10);e.setBuffer(queryLinks,offset:0,index:11);e.setBuffer(rebuildControl,offset:0,index:12);e.setBuffer(queryAlternate,offset:0,index:14);e.setBuffer(matchOrder,offset:0,index:15)
 var k=sortK,d=sortD;withUnsafeBytes(of:&k){e.setBytes($0.baseAddress!,length:4,index:16)};withUnsafeBytes(of:&d){e.setBytes($0.baseAddress!,length:4,index:17)}
 e.setBuffer(noopFlags,offset:0,index:18);e.setBuffer(queryPrevious,offset:0,index:19)
 var mask=effectMask;withUnsafeBytes(of:&mask){e.setBytes($0.baseAddress!,length:4,index:20)};e.setBuffer(effectHeads,offset:0,index:21)
 e.setBuffer(contexts,offset:0,index:22);e.setBuffer(contextInfo,offset:0,index:23)
 var phase=contextPhase;withUnsafeBytes(of:&phase){e.setBytes($0.baseAddress!,length:4,index:24)};e.setBuffer(contextResults,offset:0,index:25)
 if let contextExport=contextExport{e.setBuffer(contextExport,offset:0,index:26)}
 e.setBuffer(batchState,offset:0,index:28);e.setBuffer(batchRows,offset:0,index:29)
 var mode=rebuildMode;withUnsafeBytes(of:&mode){e.setBytes($0.baseAddress!,length:4,index:27)}
 var bit=radixBit;withUnsafeBytes(of:&bit){e.setBytes($0.baseAddress!,length:4,index:13)}
 var set=arg;withUnsafeBytes(of:&set){e.setBytes($0.baseAddress!,length:4,index:7)}
 }
 if let offset=batchIndirectOffset{e.dispatchThreadgroups(indirectBuffer:batchState,indirectBufferOffset:offset,threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))}
 else if indirect{e.dispatchThreadgroups(indirectBuffer:indirectOffset == -1 ? contextInfo : rebuildControl,indirectBufferOffset:indirectOffset == -1 ? 8 : indirectOffset,threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))}
 else{e.dispatchThreads(MTLSize(width:threads,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(threads,256,pipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1))}
 if sharedEncoder != nil{e.memoryBarrier(scope:.buffers)}else{e.endEncoding()}
}
func copyStatus(_ cb:MTLCommandBuffer,_ status:MTLBuffer,_ flags:MTLBuffer){
 let b=cb.makeBlitCommandEncoder()!;b.copy(from:meta,sourceOffset:0,to:status,destinationOffset:0,size:36);b.copy(from:rows,sourceOffset:(program["index_counter_offset"] as! Int)*8,to:status,destinationOffset:36,size:8);b.copy(from:rebuildControl,sourceOffset:0,to:flags,destinationOffset:0,size:16);b.copy(from:rows,sourceOffset:(program["query_dirty_offset"] as! Int)*8,to:status,destinationOffset:44,size:8);b.copy(from:rows,sourceOffset:((program["index_counter_offset"] as! Int)-(program["hash_capacity"] as! Int)-1)*8,to:status,destinationOffset:52,size:8);b.copy(from:rebuildControl,sourceOffset:44,to:status,destinationOffset:60,size:4);b.copy(from:noopFlags,sourceOffset:matchCapacity*4,to:status,destinationOffset:64,size:4);b.endEncoding()
}
try FileHandle.standardError.write(contentsOf:Data("device allocated_bytes=\(device.currentAllocatedSize)\n".utf8))
let input=FileHandle.standardInput,output=FileHandle.standardOutput
func read(_ n:Int)throws->Data{var d=Data();while d.count<n{guard let p=try input.read(upToCount:n-d.count),!p.isEmpty else{throw NSError(domain:"truncated command",code:1)};d+=p};return d}
func send(_ x:[UInt32])throws{try x.withUnsafeBytes{try output.write(contentsOf:Data($0))}}
var hostMeta=initialMeta
var needsIndexReset=true
var needsIndexPatch=false
var fullIndexPhases=0,patchIndexPhases=0
var needsRowIndex=true
var rebuildWaits=0,rebuildIterations=0
var contextCount=0,contextFallbacks=0
var lastContextCount=0,lastContextOverflow=0
func finishRebuild(_ arg:UInt32,_ status:MTLBuffer,_ flags:MTLBuffer)throws->Double{
  var rebuildSeconds=0.0
  var controlWords=Array(UnsafeBufferPointer(start:flags.contents().bindMemory(to:UInt32.self,capacity:4),count:4))
  let needed=controlWords[0] != 0
  while controlWords[0] != 0 && controlWords[3] == 0 {
   let cb=queue.makeCommandBuffer()!
   if rebuildBatch>1 {
    let mode=controlWords[0]
    let encoder=cb.makeComputeCommandEncoder()!
    for iteration in 0..<rebuildBatch {
     encode(cb,rebuildDispatch,1,arg,sharedEncoder:encoder,bindResources:iteration==0,rebuildMode:mode)
     // Repeat only the current branch. A mode change ends active work until
     // the next host submission; no unused alternate branch is dispatched.
     if mode==1 {
      for (i,offset) in [(0,88),(1,64),(2,76),(3,76),(4,76),(5,88),(6,76)]{
       encode(cb,rebuildPipelines[i],1,arg,indirect:true,indirectOffset:offset,sharedEncoder:encoder,bindResources:false)
      }
     }else{
      for (i,offset) in [(0,124),(1,100),(2,112),(3,124)]{
       encode(cb,rowIndexPipelines[i],1,arg,indirect:true,indirectOffset:offset,sharedEncoder:encoder,bindResources:false)
      }
     }
     encode(cb,rebuildPipelines[7],1,arg,indirect:true,indirectOffset:136,sharedEncoder:encoder,bindResources:false)
    }
    encoder.endEncoding()
   }else if controlWords[0]==2 {
    rebuildIterations+=1
    encode(cb,rowIndexPipelines[0],1,arg);encode(cb,rowIndexPipelines[1],program["hash_capacity"] as! Int,arg);encode(cb,rowIndexPipelines[2],capacity,arg);encode(cb,rowIndexPipelines[3],1,arg);encode(cb,rebuildPipelines[7],1,arg)
   }else{
    rebuildIterations+=1
    let widths=[1,program["hash_capacity"] as! Int,capacity,capacity,((capacity+255)/256)*256,1,capacity,1]
    for i in 0..<8{encode(cb,rebuildPipelines[i],widths[i],arg)}
   }
   let b=cb.makeBlitCommandEncoder()!;b.copy(from:rebuildControl,sourceOffset:0,to:flags,destinationOffset:0,size:64);b.endEncoding();cb.commit();cb.waitUntilCompleted();rebuildWaits+=1
   try need(cb.status == .completed,"parallel rebuild failed: \(String(describing:cb.error))")
   rebuildSeconds+=cb.gpuEndTime-cb.gpuStartTime
   controlWords=Array(UnsafeBufferPointer(start:flags.contents().bindMemory(to:UInt32.self,capacity:4),count:4))
  }
  if needed{rebuildIterations += rebuildBatch>1 ? Int(flags.contents().bindMemory(to:UInt32.self,capacity:16)[14]) : 0}
  if needed {
   let cb=queue.makeCommandBuffer()!
   try need(rebuildPipelines[8].maxTotalThreadsPerThreadgroup>=256,"compaction requires 256 threads per group")
   for (i,width) in [((capacity+255)/256)*256,1,capacity,capacity*stride].enumerated(){encode(cb,rebuildPipelines[8+i],width,arg)}
   copyStatus(cb,status,flags);cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"rebuild finish failed")
   rebuildSeconds+=cb.gpuEndTime-cb.gpuStartTime
  }
  return rebuildSeconds
}
// Saturate-scoped execution: all recurrence and effect discovery stays private
// until the final table publication. No binding matrix crosses the CPU boundary.
let msDescription=program["matrix_saturate"] as? [String:Any]
let msEnabled=msDescription?["enabled"] as? Bool ?? false
let msNames=["ms_count","ms_count_offsets","ms_init","ms_seed","ms_pack","ms_step_begin","ms_gather","ms_mma","ms_apply","ms_step_finish","ms_output_scan","ms_output_offsets","ms_output_scatter","ms_sort","ms_publish","ms_index","ms_commit"]
let msPipelines=try (msEnabled ? msNames:[]).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let msPlans=msDescription?["programs"] as? [[String:Any]] ?? []
let msWeights=shared((msDescription?["weights"] as? [NSNumber] ?? []).map{$0.uint16Value})
var msCalls=0,msLogicalRuns=0,msPublished=0,msPeakBytes=0
struct MatrixSatResult {let accepted:Bool;let reason:UInt32;let info:[UInt32];let history:[UInt32];let phases:[Double];let indexCount:Int64;let markerCount:Int;let scratchBytes:Int}
func matrixSaturate(_ set:UInt32)throws->MatrixSatResult{
 try need(hostMeta[4]==0,"cannot saturate a failed device state")
 guard let plan=msPlans.first(where:{($0["set"] as! Int)==Int(set)}) else{throw NSError(domain:"uncertified Saturate request",code:1)}
 let predicates=(plan["predicates"] as! [String]).count,nrows=Int(hostMeta[0]),idExtent=Int(hostMeta[1]),nfacts=predicates*idExtent
 try need(nfacts>0 && nfacts<((1<<32)-1)/2,"matrix coordinate width")
 let width=msDescription!["width"] as! Int,heads=msDescription!["heads"] as! Int,record=msDescription!["record_stride"] as! Int
 var cfg:[UInt32]=[set,UInt32(nrows),UInt32(idExtent),UInt32(nfacts),0,0,0,ProcessInfo.processInfo.environment["EGGFUSE_MATRIX_AUDIT"]=="1" ? 1:0]
 let ctrl=privateBuffer(64),readback=shared([UInt32](repeating:0,count:16))
 let prefix=privateBuffer(max(nrows,nfacts)*4),blocks=privateBuffer(max(1,(max(nrows,nfacts)+255)/256)*4)
 var gpuPhases=[Double](repeating:0,count:3),scratch=ctrl.length+prefix.length+blocks.length
 func launch(_ cb:MTLCommandBuffer,_ index:Int,_ buffers:[MTLBuffer],_ n:Int,_ grouped:Bool=false,_ stage:[UInt32]?=nil){
  if n==0{return};let e=cb.makeComputeCommandEncoder()!,pipeline=msPipelines[index];e.setComputePipelineState(pipeline);e.label=msNames[index]
  for (i,b) in [rows,meta,watermarks,ctrl].enumerated(){e.setBuffer(b,offset:0,index:i)}
  cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:4)}
  for (i,b) in buffers.enumerated(){e.setBuffer(b,offset:0,index:5+i)}
  if let stage=stage{stage.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:5+buffers.count)}}
  if grouped{e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
  else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))};e.endEncoding()
 }
 func wait(_ cb:MTLCommandBuffer,_ phase:Int)throws->[UInt32]{
  let b=cb.makeBlitCommandEncoder()!;b.copy(from:ctrl,sourceOffset:0,to:readback,destinationOffset:0,size:64);b.endEncoding();cb.commit();cb.waitUntilCompleted()
  try need(cb.status == .completed,"matrix Saturate device failure: \(String(describing:cb.error))");gpuPhases[phase]+=cb.gpuEndTime-cb.gpuStartTime
  return Array(UnsafeBufferPointer(start:readback.contents().bindMemory(to:UInt32.self,capacity:16),count:16))
 }
 let count=queue.makeCommandBuffer()!;let zero=count.makeBlitCommandEncoder()!;zero.fill(buffer:ctrl,range:0..<64,value:0);zero.endEncoding()
 launch(count,0,[prefix,blocks],max(256,(nrows+255)/256*256));launch(count,1,[blocks],1)
 var info=try wait(count,0);let markersCount=Int(info[0]);cfg[4]=UInt32(markersCount)
 try need(markersCount<((1<<32)-1)/max(record,width,heads),"marker matrix address width")
 let markers=privateBuffer(markersCount*record*4),hidden=privateBuffer(nfacts*4),birth=privateBuffer(nfacts*4),stamp=privateBuffer(nfacts*4),fired=privateBuffer(markersCount*4)
 var current=privateBuffer(nfacts*4),next=privateBuffer(nfacts*4)
 let x=privateBuffer(markersCount*width*2),y=privateBuffer(markersCount*heads*4),history=privateBuffer((nfacts+1)*8)
 scratch += [markers,hidden,birth,stamp,fired,current,next,x,y,history].map{$0.length}.reduce(0,+)
 let initial=queue.makeCommandBuffer()!;launch(initial,2,[current,hidden,birth,stamp,fired],max(nfacts,markersCount));launch(initial,3,[current,hidden,stamp],nrows);launch(initial,4,[prefix,blocks,markers],nrows);info=try wait(initial,0)
 while info[3]==0{
  let cb=queue.makeCommandBuffer()!
  for _ in 0..<8{
   launch(cb,5,[current,next],nfacts);launch(cb,6,[current,markers,x],markersCount)
   launch(cb,7,[x,msWeights,y],((markersCount+7)/8)*(heads/8),true)
   launch(cb,8,[current,next,markers,y,birth,hidden,stamp,fired],markersCount);launch(cb,9,[history],1);swap(&current,&next)
  }
  info=try wait(cb,1)
 }
 let rounds=Int(info[1]),new=Int(info[2]);var accepted=info[4]==0;var reason=info[4]
 if UInt64(hostMeta[2])+UInt64(rounds)>=UInt64(UInt32.max){accepted=false;reason=4}
 if Int(hostMeta[0])+new>capacity||Int(hostMeta[1])+new>ids||Int(hostMeta[8])+new>ids{accepted=false;reason=5}
 let histRead=shared([UInt32](repeating:0,count:max(2,rounds*2)));let readHistory=queue.makeCommandBuffer()!,h=readHistory.makeBlitCommandEncoder()!
 h.copy(from:history,sourceOffset:0,to:histRead,destinationOffset:0,size:rounds*8);h.endEncoding();readHistory.commit();readHistory.waitUntilCompleted();try need(readHistory.status == .completed,"matrix history read failed")
 let historyWords=Array(UnsafeBufferPointer(start:histRead.contents().bindMemory(to:UInt32.self,capacity:max(2,rounds*2)),count:rounds*2))
 if !accepted{return MatrixSatResult(accepted:false,reason:reason,info:hostMeta,history:[],phases:gpuPhases,indexCount:0,markerCount:markersCount,scratchBytes:scratch)}
 cfg[5]=UInt32(new);var padded=1;while padded<new{padded*=2};cfg[6]=UInt32(padded)
 let order=privateBuffer(padded*4);scratch+=order.length
 let pub=queue.makeCommandBuffer()!
 if new>0{
  launch(pub,10,[birth,prefix,blocks],(nfacts+255)/256*256);launch(pub,11,[blocks,order],padded);launch(pub,12,[birth,prefix,blocks,order],nfacts)
  var k=2;while k<=padded{var d=k/2;while d>0{launch(pub,13,[order,birth],padded,false,[UInt32(k),UInt32(d)]);d/=2};k*=2}
 }
 info=try wait(pub,2);try need(info[4]==0,"matrix effect decode failed before publication")
 let commit=queue.makeCommandBuffer()!
 if new>0&&needsRowIndex{encode(commit,rowIndexPipelines[0],1,set);encode(commit,rowIndexPipelines[1],program["hash_capacity"] as! Int,set);encode(commit,rowIndexPipelines[2],max(1,nrows),set);encode(commit,rowIndexPipelines[3],1,set)}
 launch(commit,14,[order,birth,parent,factParent],new);launch(commit,15,[order],new);launch(commit,16,[history],1)
 let status=shared([UInt32](repeating:0,count:17)),flags=privateBuffer(64);copyStatus(commit,status,flags)
 commit.commit();commit.waitUntilCompleted();try need(commit.status == .completed,"matrix publication failed");gpuPhases[2]+=commit.gpuEndTime-commit.gpuStartTime
 let result=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:16),count:9))
 let indexCount=status.contents().advanced(by:36).loadUnaligned(as:Int64.self)
 if new>0{needsRowIndex=false}
 msCalls+=1;msLogicalRuns+=rounds;msPublished+=new;msPeakBytes=max(msPeakBytes,scratch)
 return MatrixSatResult(accepted:true,reason:0,info:result,history:historyWords,phases:gpuPhases,indexCount:indexCount,markerCount:markersCount,scratchBytes:scratch)
}

let cuDescription=program["comparison_saturate"] as? [String:Any]
let cuPlans=cuDescription?["programs"] as? [[String:Any]] ?? []
let cuNames=["cu_count","cu_pack","cu_reset","cu_reduce","cu_gather","cu_mma","cu_apply","cu_congruence","cu_finish","cu_publish","cu_priority"]
let cuPipelines=try (cuPlans.isEmpty ? []:cuNames).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let cuWeights=shared((cuDescription?["weights"] as? [NSNumber] ?? []).map{Float16($0.floatValue).bitPattern})
var cuCalls=0,cuIterations=0,cuUnions=0
// Reuse allocation, never stale closure contents. Every invocation repacks
// current row ports and copies the current canonical UF before inference.
var cuBuffers:[String:MTLBuffer]=[:]
func cuBuffer(_ name:String,_ bytes:Int)->MTLBuffer{
 if let b=cuBuffers[name],b.length>=max(4,bytes){return b}
 let b=privateBuffer(bytes);cuBuffers[name]=b;return b
}
struct ComparisonResult {let accepted:Bool;let reason:UInt32;let info:[UInt32];let iterations:UInt32;let unions:UInt32;let costs:UInt32;let choices:UInt32;let scratch:Int;let phases:[Double];let index:Int64}
func comparisonSaturate(_ set:UInt32,localOnly:Bool=false,sourceLists:MTLBuffer?=nil,listCapacity:Int=0,costCount:Int=0,choiceCount:Int=0,initialRanges:MTLBuffer?=nil,sourcePriority:MTLBuffer?=nil)throws->ComparisonResult{
 try need(hostMeta[4]==0 && cuPlans.contains(where:{($0["set"] as! Int)==Int(set)}),"uncertified comparison request")
 let nrows=Int(hostMeta[0]),extent=Int(hostMeta[1]);var cfg:[UInt32]=[set,UInt32(nrows),UInt32(extent),0,0,0,ProcessInfo.processInfo.environment["EGGFUSE_MATRIX_AUDIT"]=="1" ? 1:0,0]
 let ctrl=cuBuffer("control",64),readback=shared([UInt32](repeating:0,count:16));var phases=[Double](repeating:0,count:3),scratch=64
 func launch(_ cb:MTLCommandBuffer,_ index:Int,_ buffers:[MTLBuffer],_ n:Int,_ grouped:Bool=false){
  if n==0{return};let e=cb.makeComputeCommandEncoder()!;e.label=cuNames[index];e.setComputePipelineState(cuPipelines[index]);e.setBuffer(rows,offset:0,index:0);e.setBuffer(ctrl,offset:0,index:1)
  cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:2)};for (i,b) in buffers.enumerated(){e.setBuffer(b,offset:0,index:i+3)}
  if grouped{e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
  else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))};e.endEncoding()
 }
 func wait(_ cb:MTLCommandBuffer,_ phase:Int)throws->[UInt32]{
  let b=cb.makeBlitCommandEncoder()!;b.copy(from:ctrl,sourceOffset:0,to:readback,destinationOffset:0,size:64);b.endEncoding();cb.commit();cb.waitUntilCompleted()
  try need(cb.status == .completed,"comparison matrix device failure: \(String(describing:cb.error))");phases[phase]+=cb.gpuEndTime-cb.gpuStartTime
  return Array(UnsafeBufferPointer(start:readback.contents().bindMemory(to:UInt32.self,capacity:16),count:16))
 }
 let count=queue.makeCommandBuffer()!,zero=count.makeBlitCommandEncoder()!;zero.fill(buffer:ctrl,range:0..<64,value:0);zero.endEncoding();if let _=sourceLists{let values=shared([UInt32(costCount),UInt32(choiceCount)]);let b=count.makeBlitCommandEncoder()!;b.copy(from:values,sourceOffset:0,to:ctrl,destinationOffset:0,size:8);b.endEncoding()}else{launch(count,0,[],nrows)}
 var info=try wait(count,0)
 func rejected(_ reason:UInt32)->ComparisonResult{ComparisonResult(accepted:false,reason:reason,info:hostMeta,iterations:info[3],unions:0,costs:info[0],choices:info[1],scratch:scratch,phases:phases+[0,0,-1],index:0)}
 if info[6] != 0{return rejected(info[6])};if hostMeta[2]>=UInt32.max-1{return rejected(4)}
 cfg[3]=info[0];cfg[4]=info[1];let nc=Int(info[0]),na=Int(info[1]);var hashes=2;while hashes<2*na{hashes*=2};cfg[5]=UInt32(hashes)
 let local=cuBuffer("uf",extent*4),costs=cuBuffer("costs",nc*4),choices=cuBuffer("choices",na*4),ranges=cuBuffer("ranges",extent*8),heads=cuBuffer("heads",hashes*4),ports=cuBuffer("ports",na*16),x=cuBuffer("x",na*32*2),y=cuBuffer("y",na*16*4)
 let priority=cuBuffer("priority",na*4)
 scratch += [local,costs,choices,ranges,heads,ports,x,y,priority].map{$0.length}.reduce(0,+)
 let pack=queue.makeCommandBuffer()!,copy=pack.makeBlitCommandEncoder()!;copy.copy(from:parent,sourceOffset:0,to:local,destinationOffset:0,size:extent*4);if let lists=sourceLists{if nc>0{copy.copy(from:lists,sourceOffset:0,to:costs,destinationOffset:0,size:nc*4)};if na>0{copy.copy(from:lists,sourceOffset:3*listCapacity*4,to:choices,destinationOffset:0,size:na*4)}};copy.endEncoding();if sourceLists==nil{launch(pack,1,[costs,choices],nrows)};if let classes=sourcePriority{launch(pack,10,[choices,local,classes,priority],na)};info=try wait(pack,0)
 var firstIteration=true
 while info[2]==0{
  let cb=queue.makeCommandBuffer()!
  for _ in 0..<8{
   launch(cb,2,[ranges,heads],max(extent*2,hashes))
   if firstIteration,let initial=initialRanges{let b=cb.makeBlitCommandEncoder()!;b.copy(from:initial,sourceOffset:0,to:ranges,destinationOffset:0,size:extent*8);b.endEncoding()}else{launch(cb,3,[costs,local,ranges],nc)};firstIteration=false
   launch(cb,4,[choices,local,ranges,ports,x],na);launch(cb,5,[x,cuWeights,y],((na+7)/8)*2,true)
   if sourcePriority != nil{for pass in [UInt32(1),UInt32(2)]{cfg[7]=pass;launch(cb,6,[ports,y,local,ranges,priority],na)}}else{cfg[7]=0;launch(cb,6,[ports,y,local,ranges,priority],na)}
   launch(cb,7,[ports,heads,local],na);launch(cb,8,[],1)
  }
  info=try wait(cb,1)
 }
 if info[6] != 0{return rejected(info[6])}
 let pub=queue.makeCommandBuffer()!;launch(pub,9,[local,parent,meta,watermarks,rebuildControl],info[4]>0 ? extent:1)
 let status=shared([UInt32](repeating:0,count:17)),flags=shared([UInt32](repeating:0,count:16));copyStatus(pub,status,flags);pub.commit();pub.waitUntilCompleted();try need(pub.status == .completed,"comparison publication failed")
 let rhs=pub.gpuEndTime-pub.gpuStartTime;let rebuild=localOnly ? 0.0:try finishRebuild(set,status,flags);phases[2]+=rhs+rebuild
 needsIndexReset=status.contents().bindMemory(to:UInt32.self,capacity:17)[11]==1;needsIndexPatch=status.contents().bindMemory(to:UInt32.self,capacity:17)[11]==2
 needsRowIndex=status.contents().bindMemory(to:UInt32.self,capacity:17)[13] != status.contents().bindMemory(to:UInt32.self,capacity:17)[7]+1
 let result=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:17),count:9));let index=status.contents().advanced(by:36).loadUnaligned(as:Int64.self)
 cuCalls+=1;cuIterations+=Int(info[3]);cuUnions+=Int(info[4])
 return ComparisonResult(accepted:true,reason:0,info:result,iterations:info[3],unions:info[4],costs:info[0],choices:info[1],scratch:scratch,phases:phases+[rhs,rebuild,-1],index:index)
}

// JOINT_HOST
try output.write(contentsOf:Data("DVB7".utf8))
while true{
 let d=try read(8),words=d.withUnsafeBytes{Array($0.bindMemory(to:UInt32.self))};let op=words[0],arg=words[1]
 if op==0{try FileHandle.standardError.write(contentsOf:Data("comparison_sat calls=\(cuCalls) local_iterations=\(cuIterations) unions=\(cuUnions) retained_scratch=\(cuBuffers.values.map{$0.length}.reduce(0,+))\n".utf8));try FileHandle.standardError.write(contentsOf:Data("matrix_sat calls=\(msCalls) logical_runs=\(msLogicalRuns) rows=\(msPublished) scratch_peak=\(msPeakBytes)\n".utf8));try FileHandle.standardError.write(contentsOf:Data("batch_rhs proposals=\(batchTotals[0]) new_rows=\(batchTotals[1]) fallback_segments=\(batchTotals[2]) parallel_segments=\(batchTotals[3])\n".utf8));try FileHandle.standardError.write(contentsOf:Data("device index_refresh full=\(fullIndexPhases) patch=\(patchIndexPhases)\nrebuild iterations=\(rebuildIterations) waits=\(rebuildWaits)\ncontexts emitted=\(contextCount) fallback_runs=\(contextFallbacks)\n".utf8));break}
 if op==1{
  try need(Int(arg)<(program["sets"] as! [String]).count,"ruleset")
  if needsIndexReset || !(program["incremental_queries"] as! Bool){fullIndexPhases+=1}else if needsIndexPatch{patchIndexPhases+=1}
  if ProcessInfo.processInfo.environment["EGG_DEVICE_PHASE_LOG"]=="1"{try FileHandle.standardError.write(contentsOf:Data("device begin round=\(hostMeta[6]+1) set=\((program["sets"] as! [String])[Int(arg)]) rows=\(hostMeta[0])\n".utf8))}
  let parallelMatch=(program["parallel_match_sets"] as! [NSNumber])[Int(arg)].boolValue
  var commands:[MTLCommandBuffer]=[];var orderCommand:MTLCommandBuffer?=nil;var noopFused=false;var usedBatch=false
  let matchStatus=device.makeBuffer(length:36,options:.storageModeShared)!
  let contextStatus=device.makeBuffer(length:64,options:.storageModeShared)!
  let status=device.makeBuffer(length:68,options:.storageModeShared)!,flags=device.makeBuffer(length:64,options:.storageModeShared)!
  for (stage,pipeline) in [preparePipeline,stepPipelines[Int(arg)],commitPipeline].enumerated(){
   if stage==2 && parallelMatch {
    commands[1].waitUntilCompleted();try need(commands[1].status == .completed,"parallel match failed: \(String(describing:commands[1].error))")
    if ProcessInfo.processInfo.environment["EGG_DEVICE_PHASE_LOG"]=="1"{try FileHandle.standardError.write(contentsOf:Data("device matching complete\n".utf8))}
    let info=matchStatus.contents().bindMemory(to:UInt32.self,capacity:9)
    if info[4]==0 && info[5]>1 {
     var padded=256;while padded<Int(info[5]){padded*=2}
     let sorted=queue.makeCommandBuffer()!;encode(sorted,matchSortPipelines[0],padded,arg)
     var k=512
     while k<=padded{var d=k/2;while d>=256{encode(sorted,matchSortPipelines[1],padded,arg,sortK:UInt32(k),sortD:UInt32(d));d/=2};encode(sorted,matchSortPipelines[2],padded,arg,sortK:UInt32(k));k*=2}
     sorted.commit();orderCommand=sorted
     if ProcessInfo.processInfo.environment["EGG_DEVICE_PHASE_LOG"]=="1"{sorted.waitUntilCompleted();try FileHandle.standardError.write(contentsOf:Data("device sorting complete count=\(info[5]) status=\(sorted.status.rawValue)\n".utf8))}
    }
   }
   let cb=queue.makeCommandBuffer()!
   if stage==2 && parallelMatch && ((program["noop_preflight"] as! Bool)||(program["effect_dedup"] as! Bool)) {
    let info=matchStatus.contents().bindMemory(to:UInt32.self,capacity:9)
    if info[4]==0 && info[5]>0{
     var slots=2;while slots<2*Int(info[5]){slots*=2}
     if program["effect_dedup"] as! Bool{
      let clear=cb.makeBlitCommandEncoder()!;clear.fill(buffer:effectHeads,range:0..<(slots*4),value:0);clear.endEncoding()
      encode(cb,effectPipeline,Int(info[5]),arg,effectMask:UInt32(slots-1))
     }
     encode(cb,noopPipeline,Int(info[5]),arg,effectMask:UInt32(slots-1));noopFused=true
    }
   }
   if stage==1,let prefix=prefixPipelines[Int(arg)] {
    encode(cb,contextPipelines[0],1,arg)
    encode(cb,prefix,max(1,Int(hostMeta[0])),arg)
    encode(cb,contextPipelines[1],1,arg)
    if contextRidge || contextTraining{encode(cb,contextPredictPipeline,1,arg,indirect:true,indirectOffset:-1)}
    encode(cb,contextPipelines[2],1,arg,indirect:true,indirectOffset:-1)
    if contextRidge{encode(cb,contextPipelines[2],1,arg,indirect:true,indirectOffset:-1,contextPhase:0)}
    encode(cb,contextPipelines[3],1,arg)
    encode(cb,fallbackPipelines[Int(arg)]!,max(1,Int(hostMeta[0])),arg)
    let b=cb.makeBlitCommandEncoder()!;b.copy(from:contextInfo,sourceOffset:0,to:contextStatus,destinationOffset:0,size:64);b.endEncoding()
   }else if stage==2 && batchEnabled && !batchSegments[Int(arg)].isEmpty && matchStatus.contents().bindMemory(to:UInt32.self,capacity:9)[5]>=256 {
    usedBatch=true;
    let encoder=cb.makeComputeCommandEncoder()!
    encode(cb,batchPipelines[0],1,arg,sharedEncoder:encoder)
    let matched=Int(matchStatus.contents().bindMemory(to:UInt32.self,capacity:9)[5])
    encode(cb,batchPipelines[11],max(256,(matched+255)/256*256),arg,sharedEncoder:encoder)
    encode(cb,batchPipelines[12],1,arg,sharedEncoder:encoder)
    encode(cb,batchPipelines[13],max(1,matched),arg,sharedEncoder:encoder)
    let iterations=min(batchSegments[Int(arg)].filter{batchSpec[$0][2]==1}.count,matched/256)+1
    for _ in 0..<iterations {
     encode(cb,batchPipelines[1],1,arg,sharedEncoder:encoder,batchIndirectOffset:112)
     for (pipeline,offset) in [(14,76),(15,88),(2,64),(3,160),(4,76),(5,76),(6,88),(7,76),(8,76),(9,100)] {
      encode(cb,batchPipelines[pipeline],1,arg,sharedEncoder:encoder,batchIndirectOffset:offset)
     }
    }
    encode(cb,batchPipelines[10],1,arg,sharedEncoder:encoder);encoder.endEncoding()
    let b=cb.makeBlitCommandEncoder()!;b.copy(from:batchState,sourceOffset:0,to:batchStatus,destinationOffset:0,size:64);b.endEncoding()
   }else if stage==2 && ((program["parallel_rhs_sets"] as? [NSNumber])?[Int(arg)].boolValue ?? false) {
    encode(cb,unionPipelines[0],1,arg)
    encode(cb,unionPipelines[1],max(1,Int(matchStatus.contents().bindMemory(to:UInt32.self,capacity:9)[5])),arg)
    encode(cb,unionPipelines[2],1,arg)
   }else{encode(cb,pipeline,stage==1 && parallelMatch ? max(1,Int(hostMeta[0])):1,arg)}
   if stage==0{
    if program["parallel_prepare"] as! Bool {
     if needsRowIndex{encode(cb,rowIndexPipelines[0],1,arg);encode(cb,rowIndexPipelines[1],program["hash_capacity"] as! Int,arg);encode(cb,rowIndexPipelines[2],max(1,Int(hostMeta[0])),arg);encode(cb,rowIndexPipelines[3],1,arg)}
     if needsIndexReset || !(program["incremental_queries"] as! Bool){
      encode(cb,queryPipelines[0],1,arg,indirect:true,indirectOffset:32)
      encode(cb,queryPipelines[1],1,arg)
      encode(cb,queryPipelines[2],1,arg,indirect:true,indirectOffset:32)
      for bit in 0..<(program["query_radix_passes"] as! Int){
       encode(cb,queryPipelines[3],1,arg,indirect:true,radixBit:UInt32(bit))
       encode(cb,queryPipelines[4],16,arg,radixBit:UInt32(bit))
       encode(cb,queryPipelines[5],1,arg,indirect:true,radixBit:UInt32(bit))
      }
      encode(cb,prepareClearPipeline,1,arg,indirect:true,indirectOffset:32)
      encode(cb,queryPipelines[6],1,arg,indirect:true);encode(cb,queryPipelines[7],max(1,Int(hostMeta[0])),arg)
     }
    }else{encode(cb,prepareClearPipeline,1,arg,indirect:true)}
    encode(cb,prepareBuildPipeline,1,arg)
   }
   if stage==1 && parallelMatch{let b=cb.makeBlitCommandEncoder()!;b.copy(from:meta,sourceOffset:0,to:matchStatus,destinationOffset:0,size:36);b.endEncoding()}
   if stage==2{copyStatus(cb,status,flags)}
   cb.commit();commands.append(cb)
  }
  for (i,cb) in commands.enumerated(){
   cb.waitUntilCompleted()
   if ProcessInfo.processInfo.environment["EGG_DEVICE_PHASE_LOG"]=="1"{
    let line="device phase=\(i) set=\(arg) gpu_seconds=\(cb.gpuEndTime-cb.gpuStartTime) status=\(cb.status.rawValue)\n"
    try FileHandle.standardError.write(contentsOf:Data(line.utf8))
   }
   try need(cb.status == .completed,"device phase \(i) failed: \(String(describing: cb.error))")
  }
  if let sorted=orderCommand{try need(sorted.status == .completed,"binding order failed")}
  if prefixPipelines[Int(arg)] != nil {
   let info=contextStatus.contents().bindMemory(to:UInt32.self,capacity:16)
   contextCount+=Int(info[0]);contextFallbacks+=info[1] == 0 ? 0 : 1
   lastContextCount=info[1] == 0 ? Int(info[0]) : 0;lastContextOverflow=Int(info[1])
  }else{lastContextCount=0;lastContextOverflow=0}
  if usedBatch {
   let b=batchStatus.contents().bindMemory(to:UInt32.self,capacity:16)
   for (i,j) in [9,10,11,12].enumerated(){batchTotals[i]+=UInt64(b[j])}
  }
  let rebuildSeconds=try finishRebuild(arg,status,flags)
  needsIndexReset=status.contents().bindMemory(to:UInt32.self,capacity:15)[11] == 1
  needsIndexPatch=status.contents().bindMemory(to:UInt32.self,capacity:15)[11] == 2
  needsRowIndex=status.contents().bindMemory(to:UInt32.self,capacity:15)[13] != status.contents().bindMemory(to:UInt32.self,capacity:15)[7]+1
  hostMeta=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:9),count:9));try send(hostMeta)
  // The no-op filter and RHS share a command buffer. Do not add an otherwise
  // unnecessary submission merely to obtain a separate GPU timestamp.
  var seconds=commands.map{$0.gpuEndTime-$0.gpuStartTime};let rhsSeconds=seconds[2];seconds[2]+=rebuildSeconds;if let sorted=orderCommand{seconds[1]+=sorted.gpuEndTime-sorted.gpuStartTime};seconds.append(rhsSeconds);seconds.append(rebuildSeconds);seconds.append(noopFused ? -1 : 0);try seconds.withUnsafeBytes{try output.write(contentsOf:Data($0))};try output.write(contentsOf:Data(bytes:status.contents().advanced(by:36),count:8));try output.write(contentsOf:Data(bytes:status.contents().advanced(by:60),count:8))
 }else if op==6{
  let result=try jointPolyspine();hostMeta=result.info;try send(hostMeta)
  try result.phases.withUnsafeBytes{try output.write(contentsOf:Data($0))};var index:Int64=0;try withUnsafeBytes(of:&index){try output.write(contentsOf:Data($0))};try send([0,0])
  let bytes=UInt64(result.scratch);try send([result.accepted ? 1:0,result.reason,UInt32(result.history.count),UInt32(result.statistics[11]),UInt32(result.statistics[12]),UInt32(result.statistics[88]),UInt32(truncatingIfNeeded:bytes),UInt32(bytes>>32)])
  var transfer=UInt64(result.transferBytes);try withUnsafeBytes(of:&transfer){try output.write(contentsOf:Data($0))};try result.statistics.withUnsafeBytes{try output.write(contentsOf:Data($0))}
  for step in result.history{try send(step)}
 }else if op==5{
  let result=try comparisonSaturate(arg);hostMeta=result.info;try send(hostMeta)
  try result.phases.withUnsafeBytes{try output.write(contentsOf:Data($0))};var index=result.index;try withUnsafeBytes(of:&index){try output.write(contentsOf:Data($0))};try send([0,0])
  let bytes=UInt64(result.scratch);try send([result.accepted ? 1:0,result.reason,result.iterations,result.unions,result.costs,result.choices,UInt32(truncatingIfNeeded:bytes),UInt32(bytes>>32)])
 }else if op==4{
  let result=try matrixSaturate(arg);hostMeta=result.info;try send(hostMeta)
  let seconds=result.phases+[result.phases[2],0.0,-1.0];try seconds.withUnsafeBytes{try output.write(contentsOf:Data($0))}
  var index=result.indexCount;try withUnsafeBytes(of:&index){try output.write(contentsOf:Data($0))};try send([0,0])
  let bytes=UInt64(result.scratchBytes)
  try send([result.accepted ? 1:0,result.reason,UInt32(result.history.count/2),UInt32(result.markerCount),result.accepted ? UInt32(result.history.enumerated().filter{$0.offset%2==0}.map{$0.element}.reduce(0,+)):0,UInt32(truncatingIfNeeded:bytes),UInt32(bytes>>32),0])
  try send(result.history)
 }else if op==2{
  // Explicit checkpoint only. No table contents are read by the control loop.
  let cb=queue.makeCommandBuffer()!,b=cb.makeBlitCommandEncoder()!
  let setHeader=device.makeBuffer(length:8,options:.storageModeShared)!
  b.copy(from:rows,sourceOffset:capacity*stride*8,to:setHeader,destinationOffset:0,size:8)
  let bytes=Int(hostMeta[0])*stride*8,psize=Int(hostMeta[1])*4,fsize=Int(hostMeta[8])*4
  let r=device.makeBuffer(length:max(8,bytes),options:.storageModeShared)!,p=device.makeBuffer(length:max(8,psize),options:.storageModeShared)!,f=device.makeBuffer(length:max(8,fsize),options:.storageModeShared)!
  if bytes>0{b.copy(from:rows,sourceOffset:0,to:r,destinationOffset:0,size:bytes)}
  b.copy(from:parent,sourceOffset:0,to:p,destinationOffset:0,size:psize);b.copy(from:factParent,sourceOffset:0,to:f,destinationOffset:0,size:fsize);b.endEncoding();cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"snapshot failed")
  let liveSets=Int(setHeader.contents().load(as:Int64.self));try need(liveSets>=2 && liveSets<=setWords,"invalid set arena length")
  let setSnapshot=device.makeBuffer(length:liveSets*8,options:.storageModeShared)!,scb=queue.makeCommandBuffer()!,sb=scb.makeBlitCommandEncoder()!
  sb.copy(from:rows,sourceOffset:capacity*stride*8,to:setSnapshot,destinationOffset:0,size:liveSets*8);sb.endEncoding();scb.commit();scb.waitUntilCompleted();try need(scb.status == .completed,"set snapshot failed")
  try send(hostMeta);try output.write(contentsOf:Data(bytes:r.contents(),count:bytes));try output.write(contentsOf:Data(bytes:p.contents(),count:psize));try output.write(contentsOf:Data(bytes:f.contents(),count:fsize));try send([UInt32(liveSets)]);try output.write(contentsOf:Data(bytes:setSnapshot.contents(),count:liveSets*8))
 }else if op==3 {
  try need(contextTraining,"context export requires diagnostic mode")
  let width=(program["context_ridge"] as! [String:Any])["feature_width"] as! Int
  let bytes=lastContextCount*(width+2)*4
  try send([UInt32(lastContextCount),UInt32(width),hostMeta[2],UInt32(lastContextOverflow)])
  if bytes>0 {
   let data=device.makeBuffer(length:bytes,options:.storageModeShared)!,cb=queue.makeCommandBuffer()!
   encode(cb,contextExportPipeline,lastContextCount,0,contextExport:data)
   cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"context export failed")
   try output.write(contentsOf:Data(bytes:data.contents(),count:bytes))
  }
 }else{throw NSError(domain:"unknown control command",code:1)}
}
