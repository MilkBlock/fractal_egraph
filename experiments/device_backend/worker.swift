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
let preparePipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"prepare")!)
let prepareClearPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"prepare_clear")!)
let prepareBuildPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"prepare_build")!)
let commitPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"commit_effects")!)
let noopPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"check_empty_effects")!)
let effectPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"group_same_effects")!)
let rebuildPipelines=try ["rebuild_start","rebuild_canonical","rebuild_insert","rebuild_assign","merge_scan","merge_offsets","merge_scatter","rebuild_merge","compact_scan","compact_offsets","compact_copy","compact_store"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
try need(rebuildPipelines[4].maxTotalThreadsPerThreadgroup>=256 && rebuildPipelines[8].maxTotalThreadsPerThreadgroup>=256,"rebuild scans require 256-thread groups")
let rowIndexPipelines=try ["row_index_context","row_index_clear","row_index_parallel","row_index_ready"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let queryPipelines=try ["query_row_scan","query_row_offsets","query_pairs","query_radix_scan","query_radix_offsets","query_radix_scatter","query_link_sorted","query_table_time"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
try need(queryPipelines[0].maxTotalThreadsPerThreadgroup>=256 && queryPipelines[3].maxTotalThreadsPerThreadgroup>=256,"index scans require 256-thread groups")
let matchSortPipelines=try ["match_order_local","match_order_global","match_order_merge"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
try need(matchSortPipelines[0].maxTotalThreadsPerThreadgroup>=256 && matchSortPipelines[2].maxTotalThreadsPerThreadgroup>=256,"matching order requires 256-thread groups")
let stepPipelines=try (program["kernel_names"] as! [String]).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let capacity=program["capacity"] as! Int,stride=program["stride"] as! Int,ids=program["id_capacity"] as! Int
let matchCapacity=program["match_capacity"] as! Int,matchStride=program["match_stride"] as! Int,nrules=program["rules"] as! Int
func privateBuffer(_ bytes:Int)->MTLBuffer{device.makeBuffer(length:max(8,bytes),options:.storageModePrivate)!}
func shared<T>(_ x:[T])->MTLBuffer{x.withUnsafeBytes{b in b.isEmpty ? device.makeBuffer(length:8,options:.storageModeShared)! : device.makeBuffer(bytes:b.baseAddress!,length:b.count,options:.storageModeShared)!}}
let setWords=program["set_words"] as! Int
let rows=privateBuffer((capacity*stride+setWords+(program["index_words"] as! Int))*8),parent=privateBuffer((ids+(program["union_words"] as! Int))*4),factParent=privateBuffer(ids*4)
let old=rows
let meta=privateBuffer(36),matches=privateBuffer((program["workspace_words"] as! Int)*8),watermarks=privateBuffer(nrules*4)
let instructions=privateBuffer(max(8,(program["action_code"] as! [NSNumber]).count*8)),registers=privateBuffer((program["register_count"] as! Int)*8)
let queryHeads=privateBuffer((program["query_head_words"] as! Int)*4),queryLinks=privateBuffer((program["query_link_words"] as! Int)*4)
let queryPrevious=privateBuffer((program["query_prev_words"] as! Int)*4)
let rebuildControl=privateBuffer(48)
let queryAlternate=privateBuffer((program["query_sort_words"] as! Int)*8)
let matchOrder=privateBuffer((program["match_order_capacity"] as! Int)*4)
let noopFlags=privateBuffer((matchCapacity+1)*4)
let effectHeads=privateBuffer(2*(program["match_order_capacity"] as! Int)*4)
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
rb.endEncoding();restore.commit();restore.waitUntilCompleted();try need(restore.status == .completed,"restore failed")
withExtendedLifetime(stages) {}
}
try initializeState()
program.removeValue(forKey:"initial_rows");program.removeValue(forKey:"initial_sets");program.removeValue(forKey:"action_code");for n in ["initial_parents","initial_fact_parents","initial_watermarks"]{program.removeValue(forKey:n)}
func encode(_ cb:MTLCommandBuffer,_ pipeline:MTLComputePipelineState,_ threads:Int,_ arg:UInt32,indirect:Bool=false,indirectOffset:Int=16,radixBit:UInt32=0,sortK:UInt32=0,sortD:UInt32=0,effectMask:UInt32=1){
 let e=cb.makeComputeCommandEncoder()!;e.setComputePipelineState(pipeline)
 for (i,b) in [rows,old,parent,factParent,meta,matches,watermarks].enumerated(){e.setBuffer(b,offset:0,index:i)}
 e.setBuffer(instructions,offset:0,index:8);e.setBuffer(registers,offset:0,index:9);e.setBuffer(queryHeads,offset:0,index:10);e.setBuffer(queryLinks,offset:0,index:11);e.setBuffer(rebuildControl,offset:0,index:12);e.setBuffer(queryAlternate,offset:0,index:14);e.setBuffer(matchOrder,offset:0,index:15)
 var k=sortK,d=sortD;withUnsafeBytes(of:&k){e.setBytes($0.baseAddress!,length:4,index:16)};withUnsafeBytes(of:&d){e.setBytes($0.baseAddress!,length:4,index:17)}
 e.setBuffer(noopFlags,offset:0,index:18);e.setBuffer(queryPrevious,offset:0,index:19)
 var mask=effectMask;withUnsafeBytes(of:&mask){e.setBytes($0.baseAddress!,length:4,index:20)};e.setBuffer(effectHeads,offset:0,index:21)
 var bit=radixBit;withUnsafeBytes(of:&bit){e.setBytes($0.baseAddress!,length:4,index:13)}
 var set=arg;withUnsafeBytes(of:&set){e.setBytes($0.baseAddress!,length:4,index:7)}
 if indirect{e.dispatchThreadgroups(indirectBuffer:rebuildControl,indirectBufferOffset:indirectOffset,threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))}
 else{e.dispatchThreads(MTLSize(width:threads,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(threads,256,pipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1))};e.endEncoding()
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
try output.write(contentsOf:Data("DVB7".utf8))
while true{
 let d=try read(8),words=d.withUnsafeBytes{Array($0.bindMemory(to:UInt32.self))};let op=words[0],arg=words[1]
 if op==0{try FileHandle.standardError.write(contentsOf:Data("device index_refresh full=\(fullIndexPhases) patch=\(patchIndexPhases)\n".utf8));break}
 if op==1{
  try need(Int(arg)<(program["sets"] as! [String]).count,"ruleset")
  if needsIndexReset || !(program["incremental_queries"] as! Bool){fullIndexPhases+=1}else if needsIndexPatch{patchIndexPhases+=1}
  if ProcessInfo.processInfo.environment["EGG_DEVICE_PHASE_LOG"]=="1"{try FileHandle.standardError.write(contentsOf:Data("device begin round=\(hostMeta[6]+1) set=\((program["sets"] as! [String])[Int(arg)]) rows=\(hostMeta[0])\n".utf8))}
  let parallelMatch=(program["parallel_match_sets"] as! [NSNumber])[Int(arg)].boolValue
  var commands:[MTLCommandBuffer]=[];var orderCommand:MTLCommandBuffer?=nil;var noopFused=false
  let matchStatus=device.makeBuffer(length:36,options:.storageModeShared)!
  let status=device.makeBuffer(length:68,options:.storageModeShared)!,flags=device.makeBuffer(length:16,options:.storageModeShared)!
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
   encode(cb,pipeline,stage==1 && parallelMatch ? max(1,Int(hostMeta[0])):1,arg)
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
  var rebuildSeconds=0.0
  var controlWords=Array(UnsafeBufferPointer(start:flags.contents().bindMemory(to:UInt32.self,capacity:4),count:4))
  let needed=controlWords[0] != 0
  while controlWords[0] != 0 && controlWords[3] == 0 {
   let cb=queue.makeCommandBuffer()!
   if controlWords[0]==2 {
    encode(cb,rowIndexPipelines[0],1,arg);encode(cb,rowIndexPipelines[1],program["hash_capacity"] as! Int,arg);encode(cb,rowIndexPipelines[2],capacity,arg);encode(cb,rowIndexPipelines[3],1,arg);encode(cb,rebuildPipelines[7],1,arg)
   }else{
    let widths=[1,program["hash_capacity"] as! Int,capacity,capacity,((capacity+255)/256)*256,1,capacity,1]
    for i in 0..<8{encode(cb,rebuildPipelines[i],widths[i],arg)}
   }
   let b=cb.makeBlitCommandEncoder()!;b.copy(from:rebuildControl,sourceOffset:0,to:flags,destinationOffset:0,size:16);b.endEncoding();cb.commit();cb.waitUntilCompleted()
   try need(cb.status == .completed,"parallel rebuild failed: \(String(describing:cb.error))")
   rebuildSeconds+=cb.gpuEndTime-cb.gpuStartTime
   controlWords=Array(UnsafeBufferPointer(start:flags.contents().bindMemory(to:UInt32.self,capacity:4),count:4))
  }
  if needed {
   let cb=queue.makeCommandBuffer()!
   try need(rebuildPipelines[8].maxTotalThreadsPerThreadgroup>=256,"compaction requires 256 threads per group")
   for (i,width) in [((capacity+255)/256)*256,1,capacity,capacity*stride].enumerated(){encode(cb,rebuildPipelines[8+i],width,arg)}
   copyStatus(cb,status,flags);cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"rebuild finish failed")
   rebuildSeconds+=cb.gpuEndTime-cb.gpuStartTime
  }
  needsIndexReset=status.contents().bindMemory(to:UInt32.self,capacity:15)[11] == 1
  needsIndexPatch=status.contents().bindMemory(to:UInt32.self,capacity:15)[11] == 2
  needsRowIndex=status.contents().bindMemory(to:UInt32.self,capacity:15)[13] != status.contents().bindMemory(to:UInt32.self,capacity:15)[7]+1
  hostMeta=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:9),count:9));try send(hostMeta)
  // The no-op filter and RHS share a command buffer. Do not add an otherwise
  // unnecessary submission merely to obtain a separate GPU timestamp.
  var seconds=commands.map{$0.gpuEndTime-$0.gpuStartTime};let rhsSeconds=seconds[2];seconds[2]+=rebuildSeconds;if let sorted=orderCommand{seconds[1]+=sorted.gpuEndTime-sorted.gpuStartTime};seconds.append(rhsSeconds);seconds.append(rebuildSeconds);seconds.append(noopFused ? -1 : 0);try seconds.withUnsafeBytes{try output.write(contentsOf:Data($0))};try output.write(contentsOf:Data(bytes:status.contents().advanced(by:36),count:8));try output.write(contentsOf:Data(bytes:status.contents().advanced(by:60),count:8))
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
 }else{throw NSError(domain:"unknown control command",code:1)}
}
