let smNames=["sm_count","sm_offsets","ms_init","ms_seed","sm_pack","sm_begin","sm_gather","sm_mma","sm_apply","sm_finish","ms_output_scan","ms_output_offsets","ms_output_scatter","ms_sort","ms_publish","ms_index","sm_commit","sm_advance","sm_stage_count","sm_stage_offsets","sm_stage_pack","sm_dispatch"]
let smEnabled=(program["scheduled_matrix"] as? [String:Any]) != nil
let smPipelines=try (smEnabled ? smNames:[]).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
var smCalls=0,smRuns=0,smMarkers=0,smRows=0,smLastForwarded=0
func scheduledMatrix(_ programID:UInt32)throws->MatrixSatResult{
 let scheduled=program["scheduled_matrix"] as! [String:Any]
 let admitted=(scheduled["programs"] as! [[String:Any]])[Int(programID)],set=UInt32(admitted["set"] as! Int)
 let spec=admitted["control"] as! [String:Any]
 let setIDs=(admitted["rulesets"] as! [String]).map{UInt32((program["sets"] as! [String]).firstIndex(of:$0)!)}
 let setCount=(program["sets"] as! [String]).count,usedSets=shared([UInt32(setIDs.count)]+setIDs)
 let offsets=privateBuffer((setCount+1)*4),cursor=privateBuffer(setCount*4),dispatch=privateBuffer(24)
 let controlCode=shared((spec["instructions"] as! [[NSNumber]]).flatMap{$0.map{$0.uint32Value}}),stack=privateBuffer(max(1,spec["stack_depth"] as! Int)*32)
 let localWatermarks=privateBuffer(watermarks.length)
 let wcb=queue.makeCommandBuffer()!,wblit=wcb.makeBlitCommandEncoder()!;wblit.copy(from:watermarks,sourceOffset:0,to:localWatermarks,destinationOffset:0,size:watermarks.length);wblit.endEncoding();wcb.commit()
 try need(hostMeta[4]==0,"cannot saturate a failed device state")
 guard let plan=msPlans.first(where:{($0["set"] as! Int)==Int(set)}) else{throw NSError(domain:"uncertified Saturate request",code:1)}
 let predicates=(plan["predicates"] as! [String]).count,nrows=Int(hostMeta[0]),idExtent=Int(hostMeta[1]),nfacts=predicates*idExtent
 try need(nfacts>0 && nfacts<((1<<32)-1)/2,"matrix coordinate width")
 let usesIdentity=(plan["recipes"] as! [[String:Any]]).contains{($0["virtual"] as? Bool)==true}
 let scanned=nrows+(usesIdentity ? idExtent:0)
 let width=msDescription!["width"] as! Int,heads=msDescription!["heads"] as! Int,record=msDescription!["record_stride"] as! Int
 var historyCapacity=256
 var cfg:[UInt32]=[set,UInt32(nrows),UInt32(idExtent),UInt32(nfacts),0,0,0,ProcessInfo.processInfo.environment["EGGFUSE_MATRIX_AUDIT"]=="1" ? 1:0,UInt32(scanned),UInt32(historyCapacity)]
 let ctrl=privateBuffer(64),readback=shared([UInt32](repeating:0,count:16))
 let prefix=privateBuffer(max(scanned,nfacts)*4),blocks=privateBuffer(max(1,(max(scanned,nfacts)+255)/256)*4)
 var gpuPhases=[Double](repeating:0,count:3),scratch=ctrl.length+prefix.length+blocks.length
 func launch(_ cb:MTLCommandBuffer,_ index:Int,_ buffers:[MTLBuffer],_ n:Int,_ grouped:Bool=false,_ stage:[UInt32]?=nil,_ indirect:Int?=nil){
  if n==0{return};let e=cb.makeComputeCommandEncoder()!,pipeline=smPipelines[index];e.setComputePipelineState(pipeline);e.label=smNames[index]
  for (i,b) in [rows,meta,localWatermarks,ctrl].enumerated(){e.setBuffer(b,offset:0,index:i)}
  cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:4)}
  for (i,b) in buffers.enumerated(){e.setBuffer(b,offset:0,index:5+i)}
  if let stage=stage{stage.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:5+buffers.count)}}
  if let byte=indirect{e.dispatchThreadgroups(indirectBuffer:dispatch,indirectBufferOffset:byte,threadsPerThreadgroup:MTLSize(width:grouped ? 32:256,height:1,depth:1))}
  else if grouped{e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
  else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))};e.endEncoding()
 }
 func wait(_ cb:MTLCommandBuffer,_ phase:Int)throws->[UInt32]{
  let b=cb.makeBlitCommandEncoder()!;b.copy(from:ctrl,sourceOffset:0,to:readback,destinationOffset:0,size:64);b.endEncoding();cb.commit();cb.waitUntilCompleted()
  try need(cb.status == .completed,"matrix Saturate device failure: \(String(describing:cb.error))");gpuPhases[phase]+=cb.gpuEndTime-cb.gpuStartTime
  return Array(UnsafeBufferPointer(start:readback.contents().bindMemory(to:UInt32.self,capacity:16),count:16))
 }
 let count=queue.makeCommandBuffer()!;let zero=count.makeBlitCommandEncoder()!;zero.fill(buffer:ctrl,range:0..<64,value:0);zero.endEncoding()
 launch(count,0,[prefix,blocks],max(256,(scanned+255)/256*256));launch(count,1,[blocks],1)
 var info=try wait(count,0);let markersCount=Int(info[0]);cfg[4]=UInt32(markersCount)
 try need(markersCount<((1<<32)-1)/max(record,width,heads),"marker matrix address width")
 let markers=privateBuffer(markersCount*record*4),hidden=privateBuffer(nfacts*4),birth=privateBuffer(nfacts*4),stamp=privateBuffer(nfacts*4),fired=privateBuffer(markersCount*4)
 let current=privateBuffer(nfacts*4),next=current
 let x=privateBuffer(markersCount*width*2),y=privateBuffer(markersCount*heads*4)
 var history=privateBuffer(historyCapacity*32)
 scratch += [markers,hidden,birth,stamp,fired,current,x,y,history].map{$0.length}.reduce(0,+)
 let initial=queue.makeCommandBuffer()!;launch(initial,2,[current,hidden,birth,stamp,fired],max(nfacts,markersCount));launch(initial,3,[current,hidden,stamp],nrows);launch(initial,4,[prefix,blocks,markers],scanned);info=try wait(initial,0)
 let setup=queue.makeCommandBuffer()!,z=setup.makeBlitCommandEncoder()!;z.fill(buffer:offsets,range:0..<offsets.length,value:0);z.endEncoding()
 launch(setup,18,[markers,usedSets,offsets],markersCount);launch(setup,19,[offsets,cursor],1,false,[UInt32(setCount)])
 info=try wait(setup,0);let rowMap=privateBuffer(Int(info[0])*4)
 let mapping=queue.makeCommandBuffer()!;launch(mapping,20,[markers,usedSets,cursor,rowMap],markersCount);info=try wait(mapping,0)
 scratch += usedSets.length+offsets.length+cursor.length+dispatch.length+rowMap.length
 while info[3]==0{
  let cb=queue.makeCommandBuffer()!
  if Int(info[1])+8>historyCapacity{historyCapacity*=2;let nextHistory=privateBuffer(historyCapacity*32),copy=cb.makeBlitCommandEncoder()!;copy.copy(from:history,sourceOffset:0,to:nextHistory,destinationOffset:0,size:history.length);copy.endEncoding();history=nextHistory;cfg[9]=UInt32(historyCapacity)}
  for _ in 0..<8{
   launch(cb,17,[controlCode,stack],1);launch(cb,21,[offsets,dispatch],1)
   launch(cb,5,[],1);launch(cb,6,[current,markers,x,offsets,rowMap],1,false,nil,0)
   launch(cb,7,[x,msWeights,y,offsets],1,true,nil,12)
   launch(cb,8,[current,next,markers,y,birth,hidden,stamp,fired,offsets,rowMap,x],1,false,nil,0);launch(cb,9,[history,stack],1)
  }
  info=try wait(cb,1)
 }
 smLastForwarded=Int(info[15]);let rounds=Int(info[1]),new=Int(info[2]);var accepted=info[4]==0;var reason=info[4]
 if UInt64(hostMeta[2])+UInt64(rounds)>=UInt64(UInt32.max){accepted=false;reason=4}
 if Int(hostMeta[0])+new>capacity||Int(hostMeta[1])+new>ids||Int(hostMeta[8])+new>ids{accepted=false;reason=5}
 let histRead=shared([UInt32](repeating:0,count:max(2,rounds*8)));let readHistory=queue.makeCommandBuffer()!,h=readHistory.makeBlitCommandEncoder()!
 h.copy(from:history,sourceOffset:0,to:histRead,destinationOffset:0,size:rounds*32);h.endEncoding();readHistory.commit();readHistory.waitUntilCompleted();try need(readHistory.status == .completed,"matrix history read failed")
 let historyWords=Array(UnsafeBufferPointer(start:histRead.contents().bindMemory(to:UInt32.self,capacity:max(2,rounds*8)),count:rounds*8))
 if !accepted{return MatrixSatResult(accepted:false,reason:reason,info:hostMeta,history:[],phases:gpuPhases,indexCount:0,markerCount:markersCount,scratchBytes:scratch+history.length-256*32+localWatermarks.length+controlCode.length+stack.length)}
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
 let status=shared([UInt32](repeating:0,count:17)),flags=privateBuffer(64);let wb=commit.makeBlitCommandEncoder()!;wb.copy(from:localWatermarks,sourceOffset:0,to:watermarks,destinationOffset:0,size:watermarks.length);wb.endEncoding();copyStatus(commit,status,flags)
 commit.commit();commit.waitUntilCompleted();try need(commit.status == .completed,"matrix publication failed");gpuPhases[2]+=commit.gpuEndTime-commit.gpuStartTime
 let result=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:16),count:9))
 let indexCount=status.contents().advanced(by:36).loadUnaligned(as:Int64.self)
 if new>0{needsRowIndex=false}
 smCalls+=1;smRuns+=rounds;smMarkers+=markersCount;smRows+=new;msLogicalRuns+=rounds;msPublished+=new;msPeakBytes=max(msPeakBytes,scratch)
 return MatrixSatResult(accepted:true,reason:0,info:result,history:historyWords,phases:gpuPhases,indexCount:indexCount,markerCount:markersCount,scratchBytes:scratch+history.length-256*32+localWatermarks.length+controlCode.length+stack.length)
}
