// Included at the worker's module scope by build.py. The existing typed action
// sink is reused on a transactional device view, not on the published tables.
let jpDescription=program["joint_polyspine"] as? [String:Any]
let jpProgram=jpDescription?["program"] as? [String:Any]
let jpNames=["jp_begin","jp_zero_maps","jp_count_range","jp_pack_range","jp_root_reset","jp_hash_clear","jp_hash_insert","jp_canonical","jp_retire","jp_csr_clear","jp_csr_count","jp_csr_offsets","jp_csr_fill","jp_prepare","jp_groups","jp_group_offsets","jp_bindings","jp_affine_mma","jp_validate","jp_sort_local","jp_sort_global","jp_commit","jp_comparison_roots","jp_advance","jp_close","jp_metrics","jp_csr_scan","jp_csr_blocks","jp_csr_add","jp_static_markers","jp_static_heads","jp_marker_offsets","jp_marker_items","jp_feature_clear","jp_candidate_summary","jp_choice_summary","jp_features","jp_feature_mma","jp_seams","jp_ridge_mma","jp_prediction_finish","jp_export_interfaces","jp_group_scan","jp_group_blocks","jp_group_add","jp_cache_static","jp_cache_signature","jp_cache_dependents","jp_cached_features","jp_cached_prediction","jp_cache_audit"]
let jpPipelines=try (jpProgram==nil ? []:jpNames).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
var jpPool:[String:MTLBuffer]=[:]
func jpBuffer(_ name:String,_ bytes:Int)->MTLBuffer{
 if let b=jpPool[name],b.length>=max(8,bytes){return b};let b=privateBuffer(bytes);jpPool[name]=b;return b
}
struct JointResult {let accepted:Bool;let reason:UInt32;let info:[UInt32];let statistics:[Int64];let history:[[UInt32]];let phases:[Double];let scratch:Int;let transferBytes:Int}
var jpCalls=0,jpFallbacks=0
func jointPolyspine()throws->JointResult{
 guard let plan=jpProgram else{throw NSError(domain:"no certified joint Polyspine program",code:1)}
 let savedRows=rows,savedOld=old,savedParent=parent,savedFacts=factParent,savedMeta=meta,savedWM=watermarks,savedHost=hostMeta
 let savedRowIndex=needsRowIndex
 let copyTargets=[savedRows,savedParent,savedFacts,savedMeta,savedWM]
 let copies=zip(["world","parents","facts","meta","watermarks"],copyTargets).map{jpBuffer($0.0,$0.1.length)}
 var committed=false
 defer{
  rows=savedRows;old=savedOld;parent=savedParent;factParent=savedFacts;meta=savedMeta;watermarks=savedWM
  if committed{needsIndexReset=true;needsIndexPatch=false;needsRowIndex=true}
  else{hostMeta=savedHost;needsIndexReset=true;needsIndexPatch=false;needsRowIndex=savedRowIndex}
 }
 var phases=[Double](repeating:0,count:3),history:[[UInt32]]=[];var stats=[Int64](repeating:0,count:128)
 let started=queue.makeCommandBuffer()!,blit=started.makeBlitCommandEncoder()!
 for (src,dst) in zip(copyTargets,copies){blit.copy(from:src,sourceOffset:0,to:dst,destinationOffset:0,size:src.length)}
 blit.endEncoding();started.commit();started.waitUntilCompleted();try need(started.status == .completed,"private Polyspine view copy failed");phases[0]+=started.gpuEndTime-started.gpuStartTime
 rows=copies[0];old=rows;parent=copies[1];factParent=copies[2];meta=copies[3];watermarks=copies[4]
 let sets=plan["set_ids"] as! [Int],recipes=plan["recipes"] as! [[String:Any]],nrecipes=recipes.count
 let numericWidth=(nrecipes*4+7)/8*8,maxOps=max(1,recipes.map{($0["operations"] as! [[Int]]).count}.max() ?? 0)
 let maxAtoms=(plan["plans"] as! [[String:Any]]).map{($0["roles"] as! [Int]).count}.max() ?? 1
 var stageRead:SCView?=nil
 let stateOffset=(program["index_counter_offset"] as! Int)+5+4*capacity
 let readback=shared([Int64](repeating:0,count:128)),metaBack=shared([UInt32](repeating:0,count:9))
 func launch(_ cb:MTLCommandBuffer,_ index:Int,_ bs:[MTLBuffer],_ n:Int,_ cfg:[UInt32]=[],_ grouped:Bool=false){
  if n==0{return};let e=cb.makeComputeCommandEncoder()!;e.label=jpNames[index];e.setComputePipelineState(jpPipelines[index])
  for (i,b) in [rows,parent,factParent,meta,watermarks].enumerated(){e.setBuffer(b,offset:0,index:i)}
  e.setBuffer(stageRead?.parts["rows"]?.buffer ?? rows,offset:0,index:30);e.setBuffer(stageRead?.parts["meta"]?.buffer ?? meta,offset:0,index:29)
  if [14,16].contains(index),let wm=stageRead?.parts["watermarks"]?.buffer{e.setBuffer(wm,offset:0,index:4)}
  for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:5+i)}
  if !cfg.isEmpty{let padded=cfg.count==3 ? cfg+[0]:cfg;padded.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:5+bs.count)}}
  if grouped{e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
  else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1))};e.endEncoding()
 }
 func wait(_ cb:MTLCommandBuffer,_ phase:Int)throws{
  let b=cb.makeBlitCommandEncoder()!;b.copy(from:rows,sourceOffset:stateOffset*8,to:readback,destinationOffset:0,size:64*8);b.copy(from:rows,sourceOffset:(stateOffset+80)*8,to:readback,destinationOffset:80*8,size:48*8);b.copy(from:meta,sourceOffset:0,to:metaBack,destinationOffset:0,size:36);b.endEncoding()
  cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"joint Polyspine kernel failure: \(String(describing:cb.error))");phases[phase]+=cb.gpuEndTime-cb.gpuStartTime
  stats=Array(UnsafeBufferPointer(start:readback.contents().bindMemory(to:Int64.self,capacity:128),count:128));hostMeta=Array(UnsafeBufferPointer(start:metaBack.contents().bindMemory(to:UInt32.self,capacity:9),count:9))
 }
 func failure(_ reason:UInt32)->JointResult{jpFallbacks+=1;return JointResult(accepted:false,reason:reason,info:savedHost,statistics:stats,history:history,phases:phases+[0,0,-1],scratch:jpPool.values.map{$0.length}.reduce(0,+),transferBytes:copyTargets.map{$0.length}.reduce(0,+))}
 let entryRows=Int(savedHost[0]),entryIDs=Int(savedHost[1]),rootmap=jpBuffer("root-map",entryIDs*4),fmap=jpBuffer("function-map",entryIDs*4),props=jpBuffer("properties",entryIDs*4)
 let begin=queue.makeCommandBuffer()!
 if savedRowIndex{encode(begin,rowIndexPipelines[0],1,UInt32(sets[0]));encode(begin,rowIndexPipelines[1],program["hash_capacity"] as! Int,UInt32(sets[0]));encode(begin,rowIndexPipelines[2],max(1,entryRows),UInt32(sets[0]));encode(begin,rowIndexPipelines[3],1,UInt32(sets[0]))}
 launch(begin,0,[],1);launch(begin,1,[rootmap,fmap,props],max(ids,entryRows));launch(begin,2,[],entryRows,[0,UInt32(entryRows)]);try wait(begin,0)
 if hostMeta[4] != 0{return failure(hostMeta[4])}
 let markerCount=Int(stats[37]),markers=jpBuffer("markers",markerCount*8*4)
 var listCapacity=max(1,Int(stats[36])),lists=privateBuffer(max(1,Int(stats[36]))*5*4)
 let rootUpper=max(1,markerCount*4+Int(stats[33]+stats[34])),roots=jpBuffer("roots",rootUpper*4)
 let pack=queue.makeCommandBuffer()!;if scEnabled{let clear=pack.makeBlitCommandEncoder()!;clear.fill(buffer:lists,range:0..<lists.length,value:0);clear.endEncoding()};launch(pack,3,[markers,lists,fmap,rootmap,roots,props],entryRows,[0,UInt32(entryRows),UInt32(listCapacity)]);try wait(pack,0)
 if hostMeta[4] != 0{return failure(hostMeta[4])}
 let rootCount=Int(stats[12]),rootstate=jpBuffer("root-state",rootCount*8*4),scores=jpBuffer("scores",rootCount*8*4)
 let model=plan["model"] as? [String:Any],training=program["polyspine_training"] as? Bool ?? false
 let featuresEnabled=model != nil || training,featureWidth=plan["feature_width"] as! Int
 let fullRefresh=program["polyspine_full_refresh"] as? Bool ?? false,refreshAudit=program["polyspine_refresh_audit"] as? Bool ?? false
 let featureRows=featuresEnabled ? rootCount:0
 let stat=jpBuffer("static-features",featureRows*featureWidth*4),heads=jpBuffer("head-sums",featureRows*64*4),headCounts=jpBuffer("head-counts",featureRows*4)
 let moffset=jpBuffer("marker-offsets",(rootCount+1)*4),mcursor=jpBuffer("marker-cursors",featureRows*4),mitems=jpBuffer("marker-items",markerCount*4)
 let initial=jpBuffer("initial-features",featureRows*4*8),baseFeatures=jpBuffer("base-features",3*featureRows*featureWidth*2),encoded=jpBuffer("encoded-features",3*featureRows*64*4),ridgeInput=jpBuffer("ridge-input",3*featureRows*136*2)
 let staticContext=jpBuffer("immutable-context",featureRows*featureWidth*2),signatures=jpBuffer("feature-signatures",3*featureRows*16*8),baseDirty=jpBuffer("base-dirty",featureRows*4),outputDirty=jpBuffer("output-dirty",featureRows*4),rawScores=jpBuffer("raw-scores",3*featureRows*8*4)
 let summary=jpBuffer("candidate-summary",featureRows*4*4)
 func halfMatrix(_ name:String)->MTLBuffer{shared((plan[name] as! [[NSNumber]]).flatMap{$0}.map{Float16($0.floatValue).bitPattern})}
 let encoding=halfMatrix("encoding"),headEncoding=halfMatrix("head_encoding"),roles=halfMatrix("role_encoding")
 let ridgeWeights=shared((model?["weights"] as? [NSNumber] ?? [NSNumber](repeating:0,count:136*8)).map{Float16($0.floatValue).bitPattern})
 let scaleValues=(model?["scales"] as? [NSNumber] ?? [1,1,1])+(model?["bias"] as? [NSNumber] ?? [0,0,0])
 let ridgeScales=shared(scaleValues.map{$0.floatValue})
 try need(ridgeScales.length>=6*MemoryLayout<Float>.size,"ridge scale/bias ABI requires six floats")
 var firstFeatures=true,trainingFrames:[(Int,UInt32,Data,Data,Data,Data)]=[]
 func download(_ fields:[(MTLBuffer,Int)],offsets:[Int]=[])throws->[Data]{
  let cb=queue.makeCommandBuffer()!,b=cb.makeBlitCommandEncoder()!;let outputs=fields.map{device.makeBuffer(length:max(8,$0.1),options:.storageModeShared)!}
  for (i,entry) in zip(fields,outputs).enumerated() where entry.0.1>0{let ((src,n),dst)=entry;b.copy(from:src,sourceOffset:offsets.isEmpty ? 0:offsets[i],to:dst,destinationOffset:0,size:n)};b.endEncoding();cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"Polyspine diagnostic download failed")
  return zip(outputs,fields).map{Data(bytes:$0.0.contents(),count:$0.1.1)}
 }
 var weights=[UInt16](repeating:0,count:numericWidth*8)
 for (i,r) in recipes.enumerated(){for (j,n) in (r["coefficients"] as! [NSNumber]).enumerated(){weights[(i*4+j)*8]=Float16(n.floatValue).bitPattern}}
 let affineWeights=shared(weights)
 let setup=queue.makeCommandBuffer()!;launch(setup,4,[rootstate,scores],rootCount);launch(setup,5,[],program["hash_capacity"] as! Int);launch(setup,6,[lists],Int(stats[17]),[UInt32(listCapacity)])
 if featuresEnabled{
  let clear=setup.makeBlitCommandEncoder()!;for b in [stat,heads,headCounts,initial,signatures]{clear.fill(buffer:b,range:0..<b.length,value:0)};clear.endEncoding()
  launch(setup,29,[markers,rootmap,stat],markerCount);launch(setup,30,[rootmap,headEncoding,heads,headCounts],entryRows);launch(setup,31,[stat,moffset,mcursor],1);launch(setup,32,[markers,rootmap,moffset,mcursor,mitems],markerCount);launch(setup,45,[props,markers,rootmap,moffset,mitems,roots,stat,staticContext],rootCount)
 };try wait(setup,0)
 if hostMeta[4] != 0{return failure(hostMeta[4])}
 func stageParts()->[String:SCPart]{[
  "rows":SCPart(buffer:rows,words:Int(hostMeta[0])*stride*2),"parents":SCPart(buffer:parent,words:Int(hostMeta[1])),
  "fact_parents":SCPart(buffer:factParent,words:Int(hostMeta[8])),"watermarks":SCPart(buffer:watermarks,words:nrules),"meta":SCPart(buffer:meta,words:9),
  "function_map":SCPart(buffer:fmap,words:entryIDs),"cost_rows":SCPart(buffer:lists,words:Int(stats[13])),"candidate_rows":SCPart(buffer:lists,words:Int(stats[14]),offset:listCapacity),
  "function_rows":SCPart(buffer:lists,words:Int(stats[15]),offset:2*listCapacity),"choice_rows":SCPart(buffer:lists,words:Int(stats[16]),offset:3*listCapacity),"mutable_rows":SCPart(buffer:lists,words:Int(stats[17]),offset:4*listCapacity),"root_state":SCPart(buffer:rootstate,words:rootCount*8)]}
 if scEnabled{try need(stats[104]>=2 && stats[104]<=setWords,"stage set arena extent invalid")}
 let stageTracker:StageContexts? = scEnabled ? try autoreleasepool{try StageContexts(stageParts(),hostMeta,listCapacity,["markers":SCPart(buffer:markers,words:markerCount*8),"roots":SCPart(buffer:roots,words:rootCount),"rootmap":SCPart(buffer:rootmap,words:entryIDs),"set_arena":SCPart(buffer:rows,words:Int(stats[104])*2,offset:capacity*stride*2)],queue:queue)}:nil
 var stageAccounted=stageTracker?.gpuSeconds ?? 0;phases[0]+=stageAccounted
 var publishedTo=entryRows
 func appendRows()throws{
  let end=Int(hostMeta[0]);if end==publishedTo{return}
  let count=queue.makeCommandBuffer()!,b=count.makeBlitCommandEncoder()!;b.fill(buffer:rows,range:(stateOffset+32)*8..<(stateOffset+38)*8,value:0);b.endEncoding();launch(count,2,[],end-publishedTo,[UInt32(publishedTo),UInt32(end)]);try wait(count,0)
  if hostMeta[4] != 0{return}
  let needed=Int(stats[17]+stats[36]);if needed>listCapacity{
   let newCapacity=max(1,needed),new=privateBuffer(newCapacity*5*4),cb=queue.makeCommandBuffer()!,copy=cb.makeBlitCommandEncoder()!
   if scEnabled{copy.fill(buffer:new,range:0..<new.length,value:0)}
   for j in 0..<5{let n=Int(stats[j==4 ? 17:13+j]);if n>0{copy.copy(from:lists,sourceOffset:j*listCapacity*4,to:new,destinationOffset:j*newCapacity*4,size:n*4)}}
   copy.endEncoding();cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"local port growth failed");phases[0]+=cb.gpuEndTime-cb.gpuStartTime;lists=new;listCapacity=newCapacity
  }
  let add=queue.makeCommandBuffer()!;launch(add,3,[markers,lists,fmap,rootmap,roots,props],end-publishedTo,[UInt32(publishedTo),UInt32(end),UInt32(listCapacity)]);try wait(add,0);publishedTo=end
  try need(Int(stats[12])==rootCount,"certified program changed its root carrier")
 }
 while Int(stats[1])<3 && hostMeta[4]==0{
  let phase=Int(stats[1]);stageRead=stageTracker?.begin(phase)
  if let view=stageRead{try need(view.metadata==hostMeta && view.listCapacity==listCapacity,"stage input version does not match its schedule boundary")}
  let readCosts=stageRead?.parts["cost_rows"]?.buffer ?? lists,readCandidates=stageRead?.parts["candidate_rows"]?.buffer ?? lists,readFmap=stageRead?.parts["function_map"]?.buffer ?? fmap
  let readMarkers=stageTracker?.interface["markers"]?.buffer ?? markers
  let extent=Int(hostMeta[1]),count=jpBuffer("cost-counts",extent*4),offsets=jpBuffer("cost-offsets",(extent+1)*4),cursors=jpBuffer("cost-cursors",extent*4),stamps=jpBuffer("cost-stamps",extent*4),ranges=jpBuffer("cost-ranges",extent*8),items=jpBuffer("cost-items",Int(stats[13])*4)
  let scans=jpBuffer("csr-scans",((extent+255)/256)*4),choiceCounts=jpBuffer("choice-counts",extent*4),priority=jpBuffer("class-priority",extent*4)
  let ngroups=phase==0 ? markerCount:Int(stats[14])*3,groups=jpBuffer("groups",ngroups*16*4)
  let groupBlocks=jpBuffer("group-blocks",((ngroups+255)/256)*4)
  let prepare=queue.makeCommandBuffer()!
  if phase<2{launch(prepare,13,[],1)}
  launch(prepare,9,[count,stamps,ranges],extent);launch(prepare,10,[readCosts,count,stamps,ranges],Int(stats[13]),[UInt32(listCapacity)]);launch(prepare,26,[count,offsets,scans],((extent+255)/256)*256);launch(prepare,27,[scans,offsets],1);launch(prepare,28,[offsets,cursors,scans],extent);launch(prepare,12,[readCosts,offsets,cursors,items],Int(stats[13]))
  if featuresEnabled{
   launch(prepare,33,[summary,choiceCounts,priority],max(extent,rootCount));launch(prepare,34,[lists,rootmap,count,ranges,summary],Int(stats[14]),[UInt32(listCapacity)]);launch(prepare,35,[lists,choiceCounts],Int(stats[16]),[UInt32(listCapacity)])
   launch(prepare,46,[roots,fmap,count,ranges,summary,choiceCounts,markers,moffset,mitems,rootstate,initial,signatures,baseDirty],rootCount,[firstFeatures ? 1:0,fullRefresh ? 1:0])
   launch(prepare,47,[markers,rootmap,moffset,mitems,baseDirty,outputDirty],rootCount)
   launch(prepare,48,[staticContext,signatures,initial,stat,baseDirty,baseFeatures],rootCount)
   launch(prepare,37,[baseFeatures,encoding,encoded,baseDirty],((rootCount+7)/8)*8,[1],true);launch(prepare,38,[encoded,heads,headCounts,markers,rootmap,moffset,mitems,roles,ridgeInput,outputDirty],rootCount,[1])
   if model != nil{launch(prepare,39,[ridgeInput,ridgeWeights,rawScores,outputDirty],(rootCount+7)/8,[1],true)}
   launch(prepare,49,[rawScores,scores,ridgeScales,roots,fmap,priority],rootCount,[model == nil ? 0:1]);firstFeatures=false
   if refreshAudit{
    let ax=jpBuffer("audit-features",rootCount*featureWidth*2),ae=jpBuffer("audit-encoding",rootCount*64*4),ai=jpBuffer("audit-input",rootCount*136*2),ar=jpBuffer("audit-raw",rootCount*8*4)
    launch(prepare,36,[roots,fmap,count,ranges,summary,choiceCounts,props,markers,rootmap,moffset,mitems,stat,rootstate,initial,ax],rootCount,[0])
    launch(prepare,37,[ax,encoding,ae,baseDirty],((rootCount+7)/8)*8,[0],true);launch(prepare,38,[ae,heads,headCounts,markers,rootmap,moffset,mitems,roles,ai,outputDirty],rootCount,[0])
    if model != nil{launch(prepare,39,[ai,ridgeWeights,ar,outputDirty],(rootCount+7)/8,[0],true)}
    launch(prepare,50,[baseFeatures,ax,encoded,ae,ridgeInput,ai,rawScores,ar],rootCount,[model == nil ? 0:1])
   }
  }
  if phase<2{launch(prepare,14,[readMarkers,readCandidates,readFmap,count,offsets,stamps,groups],ngroups,[UInt32(ngroups),stageRead == nil ? UInt32(listCapacity):0]);launch(prepare,42,[groups,groupBlocks],((ngroups+255)/256)*256,[UInt32(ngroups)]);launch(prepare,43,[groupBlocks],1,[UInt32(ngroups)]);launch(prepare,44,[groups,groupBlocks],ngroups,[UInt32(ngroups)])};try wait(prepare,0)
  if hostMeta[4] != 0{break}
  if training && !trainingFrames.contains(where:{$0.0==phase}){
   let frame=try download([(baseFeatures,rootCount*featureWidth*2),(ridgeInput,rootCount*136*2),(scores,rootCount*8*4),(rootstate,rootCount*8*4)],offsets:[phase*rootCount*featureWidth*2,phase*rootCount*136*2,0,0])
   trainingFrames.append((phase,hostMeta[2],frame[0],frame[1],frame[2],frame[3]))
  }
  if phase==2{
   let result=try comparisonSaturate(UInt32(sets[2]),localOnly:true,sourceLists:lists,listCapacity:listCapacity,costCount:Int(stats[13]),choiceCount:Int(stats[16]),initialRanges:ranges,sourcePriority:model == nil ? nil:priority,readRows:stageRead?.parts["rows"]?.buffer,readParents:stageRead?.parts["parents"]?.buffer,readCosts:stageRead?.parts["cost_rows"]?.buffer,readChoices:stageRead?.parts["choice_rows"]?.buffer)
   if !result.accepted{return failure(result.reason)};hostMeta=result.info;for i in 0..<3{phases[i]+=result.phases[i]}
   let fix=queue.makeCommandBuffer()!;launch(fix,7,[lists],Int(stats[17]),[UInt32(listCapacity)]);launch(fix,5,[],program["hash_capacity"] as! Int);launch(fix,6,[lists],Int(stats[17]),[UInt32(listCapacity)]);launch(fix,8,[lists],Int(stats[17]),[UInt32(listCapacity)]);launch(fix,7,[lists],Int(stats[17]),[UInt32(listCapacity)]);launch(fix,22,[roots,fmap,rootstate],rootCount);launch(fix,23,[],1);try wait(fix,2)
  }else{
   let jobs=Int(stats[9]),valid=jpBuffer("job-valid",jobs*4),info=jpBuffer("job-info",jobs*6*4),witnesses=jpBuffer("job-witnesses",jobs*maxAtoms*4),numeric=jpBuffer("job-numeric",jobs*maxOps*8),x=jpBuffer("job-x",jobs*numericWidth*2),y=jpBuffer("job-y",jobs*8*4)
   let compute=queue.makeCommandBuffer()!
   for pass in [1,0]{launch(compute,16,[groups,items,matches,witnesses,valid,info,x,rootmap,scores],jobs,[UInt32(ngroups),UInt32(numericWidth),model == nil ? 0:1,UInt32(pass)])}
   if phase==0{launch(compute,17,[x,affineWeights,y],(jobs+7)/8,[UInt32(numericWidth)],true)}
   launch(compute,18,[info,y,valid,numeric,matchOrder],jobs);try wait(compute,1)
   if hostMeta[4] != 0{break}
   let matched=Int(hostMeta[5]),commit=queue.makeCommandBuffer()!
   if matched>1{var padded=256;while padded<matched{padded*=2};launch(commit,19,[matches,witnesses,matchOrder],padded);var k=512;while k<=padded{var d=k/2;while d>0{launch(commit,20,[matches,witnesses,matchOrder],padded,[UInt32(k),UInt32(d)]);d/=2};k*=2}}
   // The next schedule state depends only on completed device effects. Keep
   // that transition on this command buffer; no host round-trip is needed.
   launch(commit,21,[matches,matchOrder,numeric,info,instructions,registers,rootstate,roots],1);launch(commit,23,[],1);try wait(commit,2)
   if hostMeta[4] != 0{break};try appendRows()
  }
  if let tracker=stageTracker,hostMeta[4]==0{try tracker.seal(phase,stageParts(),hostMeta,listCapacity);phases[2]+=tracker.gpuSeconds-stageAccounted;stageAccounted=tracker.gpuSeconds}
  history.append([UInt32(phase),UInt32(sets[phase]),hostMeta[2],UInt32(stats[23]),phase==2 ? UInt32.max:hostMeta[5],hostMeta[3],hostMeta[7]])
 }
 if hostMeta[4] != 0{return failure(hostMeta[4])}
 if let tracker=stageTracker{try tracker.finish(jpCalls,liveRows:rows);phases[2]+=tracker.gpuSeconds-stageAccounted}
 // Freeze immutable Interface payloads before compaction. savedRows is the
 // publication destination and no longer contains the entry after copy-back.
 let diagnosticPayload=training ? jpBuffer("diagnostic-interface",markerCount*(stride-4)*8):nil
 let diagnosticTables=training ? jpBuffer("diagnostic-interface-tables",markerCount*4):nil
 if let payload=diagnosticPayload,let tableIds=diagnosticTables{
  let source=queue.makeCommandBuffer()!;launch(source,41,[markers,payload,tableIds],markerCount);source.commit();source.waitUntilCompleted();try need(source.status == .completed,"interface diagnostic export failed")
 }
 // One publication boundary. Canonicalize all otherwise-unobserved dependent
 // rows here, then copy the exact state back; no CPU reconstruction/repair.
 let close=queue.makeCommandBuffer()!;launch(close,24,[],1)
 let flags=shared([UInt32](repeating:0,count:16)),status=shared([UInt32](repeating:0,count:17)),startRebuild=shared([UInt32](repeating:0,count:40))
 startRebuild.contents().storeBytes(of:UInt32(1),as:UInt32.self)
 let cb=close.makeBlitCommandEncoder()!;cb.copy(from:startRebuild,sourceOffset:0,to:rebuildControl,destinationOffset:0,size:160);cb.endEncoding();copyStatus(close,status,flags);close.commit();close.waitUntilCompleted();try need(close.status == .completed,"joint closure publication boundary failed");phases[2]+=close.gpuEndTime-close.gpuStartTime
 phases[2]+=try finishRebuild(UInt32(sets[2]),status,flags)
 hostMeta=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:9),count:9));if hostMeta[4] != 0{return failure(hostMeta[4])}
 let metrics=queue.makeCommandBuffer()!;launch(metrics,25,[rootstate],max(rootCount,Int(hostMeta[0])));try wait(metrics,2)
 let publish=queue.makeCommandBuffer()!,pb=publish.makeBlitCommandEncoder()!;for (src,dst) in zip(copies,copyTargets){pb.copy(from:src,sourceOffset:0,to:dst,destinationOffset:0,size:dst.length)};pb.endEncoding();publish.commit();publish.waitUntilCompleted();try need(publish.status == .completed,"joint transactional publication failed");phases[2]+=publish.gpuEndTime-publish.gpuStartTime
 committed=true
 if training{
  let payload=diagnosticPayload!,tableIds=diagnosticTables!
  let data=try download([(roots,rootCount*4),(markers,markerCount*8*4),(heads,rootCount*64*4),(headCounts,rootCount*4),(moffset,(rootCount+1)*4),(mitems,markerCount*4),(payload,markerCount*(stride-4)*8),(tableIds,markerCount*4),(rootstate,rootCount*8*4)])
  let folder=URL(fileURLWithPath:CommandLine.arguments[1]).deletingLastPathComponent().appendingPathComponent("polyspine-training")
  try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
  let manifest:[String:Any]=["schema":"resident-polyspine-training/v1","layout_sha256":plan["layout_sha256"]!,"rules_sha256":plan["rules_sha256"]!,"feature_width":featureWidth,"ridge_width":136,"heads":3,"boundary":"features frozen before each captured phase; labels are subsequent exact private effects at the joint exit","normal_inference_downloads_features":false]
  try JSONSerialization.data(withJSONObject:manifest,options:[.prettyPrinted,.sortedKeys]).write(to:folder.appendingPathComponent("manifest.json"))
  func appendWords(_ words:[UInt32],_ target:inout Data){words.withUnsafeBytes{target.append(contentsOf:$0)}}
  var aux=Data("JPSPN001".utf8);appendWords([UInt32(rootCount),UInt32(markerCount),UInt32(featureWidth),UInt32(stride-4)],&aux)
  for i in 0..<8{aux.append(data[i])};try aux.write(to:folder.appendingPathComponent("scope-\(jpCalls)-interfaces.bin"))
  for (phase,epoch,features,input,scores,prefix) in trainingFrames{
   var bytes=Data("JPRDG001".utf8);appendWords([UInt32(rootCount),UInt32(featureWidth),136,UInt32(phase),epoch,UInt32(jpCalls)],&bytes)
   for block in [data[0],features,input,scores,prefix,data[8]]{bytes.append(block)}
   let file=folder.appendingPathComponent("scope-\(jpCalls)-phase-\(phase).bin");try need(!FileManager.default.fileExists(atPath:file.path),"training frame already exists")
   try bytes.write(to:file)
  }
 }
 jpCalls+=1
 return JointResult(accepted:true,reason:0,info:hostMeta,statistics:stats,history:history,phases:phases+[phases[2],0,-1],scratch:jpPool.values.map{$0.length}.reduce(0,+)+lists.length,transferBytes:2*copyTargets.map{$0.length}.reduce(0,+))
}
