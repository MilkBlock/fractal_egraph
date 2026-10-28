// No learning is required here. Each stage reads an immutable version; only
// an exact patch can construct its successor. Buffers are never pooled while
// an older view may still be referenced by a submitted command buffer.
let scEnabled=(program["stage_cell"] as? [String:Any]) != nil
let scAudit=program["stage_context_audit"] as? Bool ?? false
let scExport=program["stage_context_export"] as? Bool ?? false
let scStages=((program["stage_cell"] as? [String:Any])?["contract"] as? [String:Any])?["stages"] as? [[String:Any]] ?? []
let scTableNames=(program["tables"] as! [[String:Any]]).map{$0["name"] as! String}
let scReadMask=shared((scEnabled ? Array(0..<4):[]).flatMap{bank -> [UInt32] in
 let names:Set<String> = bank==0 ? Set(scStages.flatMap{$0["reads"] as! [String]}):Set(scStages[bank-1]["reads"] as! [String])
 return scTableNames.map{names.contains($0) ? UInt32(1):UInt32(0)}
})
let scNames=["sc_count","sc_pack","sc_apply","sc_equal","sc_scan","sc_probe","sc_arena_audit","sc_page_init","sc_page_snapshot","sc_page_release","sc_page_flat","sc_page_audit","sc_page_changes","sc_patch_flat","sc_page_dispatch","sc_page_copy","sc_journal_list","sc_journal_dispatch"]
let scPipelines=try (scEnabled ? scNames:[]).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
var scVersion:UInt32=0
var scReports:[[String:Any]]=[]
struct SCPart {let buffer:MTLBuffer;let words:Int;let offset:Int;let rowPages:SCRowPages?
 init(buffer:MTLBuffer,words:Int,offset:Int=0,rowPages:SCRowPages?=nil){self.buffer=buffer;self.words=words;self.offset=offset;self.rowPages=rowPages}}
final class SCRowPool {
 let queue:MTLCommandQueue;var data:MTLBuffer;var free:MTLBuffer;var refs:MTLBuffer;let control=shared([UInt32](repeating:0,count:4));var capacity=0;var growthCopyBytes=0
 init(_ queue:MTLCommandQueue){self.queue=queue;data=privateBuffer(8);free=privateBuffer(8);refs=privateBuffer(8)}
 func reserve(_ pages:Int,_ cb:MTLCommandBuffer){let available=Int(control.contents().load(as:UInt32.self));if pages<=available{return}
  let n=max(capacity+pages-available,max(1,capacity+capacity/4)),d=privateBuffer(n*1024),f=privateBuffer(n*4),r=privateBuffer(n*4)
  if capacity>0{let b=cb.makeBlitCommandEncoder()!;for (a,z,bytes) in [(data,d,capacity*1024),(free,f,capacity*4),(refs,r,capacity*4)]{b.copy(from:a,sourceOffset:0,to:z,destinationOffset:0,size:bytes);growthCopyBytes+=bytes};b.endEncoding()}
  data=d;free=f;refs=r;scLaunch(cb,7,[free,refs,control],1,[UInt32(capacity),UInt32(n)]);capacity=n
 }
 func snapshot(_ live:SCPart,_ before:SCPart?,_ blocks:MTLBuffer,_ changed:Int,_ cb:MTLCommandBuffer)->SCPart{
  reserve(changed,cb);let pages=(live.words+255)/256,map=shared([UInt32(1)]+[UInt32](repeating:0,count:pages)),old=before?.rowPages?.map ?? scFlatMap
  if pages>0{let copies=privateBuffer((changed+1)*4),z=cb.makeBlitCommandEncoder()!;z.fill(buffer:copies,range:0..<4,value:0);z.endEncoding()
   let e=cb.makeComputeCommandEncoder()!;e.label="share stage pages";e.setComputePipelineState(scPipelines[8]);for (i,b) in [live.buffer,data,old,map,free,refs,control,blocks].enumerated(){e.setBuffer(b,offset:i==0 ? live.offset*4:0,index:i)};let cfg=[UInt32(before?.words ?? 0),UInt32(live.words)];cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:8,index:8)};e.setBuffer(copies,offset:0,index:9);e.dispatchThreads(MTLSize(width:pages,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));e.endEncoding()
   scLaunch(cb,15,[live.buffer,data,map,copies],changed*256,[UInt32(live.words)],offsets:[live.offset,0,0,0])
  }
  return SCPart(buffer:map,words:live.words,rowPages:SCRowPages(self,map,pages))
 }
}
final class SCRowPages {
 let pool:SCRowPool;let map:MTLBuffer;let count:Int
 init(_ pool:SCRowPool,_ map:MTLBuffer,_ count:Int){self.pool=pool;self.map=map;self.count=count}
 deinit{let cb=pool.queue.makeCommandBuffer()!;scLaunch(cb,9,[map,pool.free,pool.refs,pool.control],count,[UInt32(count)]);cb.commit()}
}
final class SCView {
 let version:UInt32;let metadata:[UInt32];let parts:[String:SCPart];let listCapacity:Int
 init(_ version:UInt32,_ metadata:[UInt32],_ parts:[String:SCPart],_ listCapacity:Int){self.version=version;self.metadata=metadata;self.parts=parts;self.listCapacity=listCapacity}
 var bytes:Int{parts.values.map{$0.buffer.length}.reduce(0,+)}
}
struct SCSeal {
 let phase:Int;let before:SCView;let after:SCView;let command:MTLCommandBuffer;let header:MTLBuffer;let error:MTLBuffer;let changes:[String:SCChange];let names:[String];let counts:MTLBuffer;let rowWords:Int
}
struct SCChange {let buffer:MTLBuffer;let count:Int;let extent:Int}
struct SCPatch {let from:UInt32;let to:UInt32;let changes:[String:SCChange];let header:MTLBuffer;var bytes:Int{changes.values.map{$0.buffer.length}.reduce(0,+)+header.length}}
func scNext()throws->UInt32{try need(scVersion<UInt32.max,"stage context version exhausted");scVersion+=1;return scVersion}
func scLaunch(_ cb:MTLCommandBuffer,_ kernel:Int,_ buffers:[MTLBuffer],_ n:Int,_ cfg:[UInt32]=[],offsets:[Int]=[]){
 if n==0{return};let e=cb.makeComputeCommandEncoder()!;e.label=scNames[kernel];e.setComputePipelineState(scPipelines[kernel])
 for (i,b) in buffers.enumerated(){e.setBuffer(b,offset:offsets.isEmpty ? 0:offsets[i]*4,index:i)}
 if !cfg.isEmpty{cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:buffers.count)}}
 let threads=(kernel<2 || kernel==13) ? ((n+255)/256)*256:n
 e.dispatchThreads(MTLSize(width:threads,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));e.endEncoding()
}
final class StageContexts {
 let queue:MTLCommandQueue
 let base:SCView;var current:SCView;var banks=[SCView?](repeating:nil,count:3);var patches=[SCPatch?](repeating:nil,count:3)
 let rowPool:SCRowPool;let pageCount=shared([UInt32(0)]);let rowCount=shared([UInt32(0)]);let pageBlocks=privateBuffer(((capacity*stride*2+255)/256)*4);let pageDispatch=privateBuffer(12);let journal=privateBuffer((((capacity*stride*2+255)/256)+1)*4);var pending:SCSeal?=nil;var sealWaits=0;let interface:[String:SCPart];let counters:[MTLBuffer];let countReadback:MTLBuffer;let empty:MTLBuffer;var blockCounts:[MTLBuffer]
 var history:[[String:Any]]=[];var gpuSeconds=0.0;var copyBytes=0;var patchWords=0;var peakBytes=0
 init(_ live:[String:SCPart],_ metadata:[UInt32],_ listCapacity:Int,_ interface:[String:SCPart],queue:MTLCommandQueue)throws{
  self.queue=queue;rowPool=SCRowPool(queue);counters=live.keys.map{_ in privateBuffer(4)};countReadback=shared([UInt32](repeating:0,count:live.count));empty=privateBuffer(8);blockCounts=live.keys.map{_ in privateBuffer(8)}
  let cb=queue.makeCommandBuffer()!,b=cb.makeBlitCommandEncoder()!;var parts:[String:SCPart]=[:],frozen:[String:SCPart]=[:]
  for (name,p) in live where name != "rows"{let dst=privateBuffer(p.words*4);if p.words>0{b.copy(from:p.buffer,sourceOffset:p.offset*4,to:dst,destinationOffset:0,size:p.words*4)};parts[name]=SCPart(buffer:dst,words:p.words);copyBytes+=p.words*4}
  for (name,p) in interface{let dst=privateBuffer(p.words*4);if p.words>0{b.copy(from:p.buffer,sourceOffset:p.offset*4,to:dst,destinationOffset:0,size:p.words*4)};frozen[name]=SCPart(buffer:dst,words:p.words);copyBytes+=p.words*4}
  b.endEncoding();let zero=cb.makeBlitCommandEncoder()!;zero.fill(buffer:counters[0],range:0..<4,value:0);zero.endEncoding();parts["rows"]=rowPool.snapshot(live["rows"]!,nil,empty,(live["rows"]!.words+255)/256,cb);cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"stage entry snapshot failed")
  base=SCView(try scNext(),metadata,parts,listCapacity);current=base;self.interface=frozen;gpuSeconds=cb.gpuEndTime-cb.gpuStartTime;account()
 }
 func account(){var seen=Set<ObjectIdentifier>();var buffers=([base,current]+banks.compactMap{$0}).flatMap{$0.parts.values.map{$0.buffer}}+interface.values.map{$0.buffer}+counters+blockCounts+[countReadback,empty,pageCount,rowCount,pageBlocks,pageDispatch,journal,rowPool.data,rowPool.free,rowPool.refs,rowPool.control]
  for p in patches.compactMap({$0}){buffers += p.changes.values.map{$0.buffer}+[p.header]}
  peakBytes=max(peakBytes,buffers.filter{seen.insert(ObjectIdentifier($0)).inserted}.map{$0.length}.reduce(0,+))}
 func begin(_ phase:Int)->SCView{banks[phase]=current;account();return current}
 func wait(_ cb:MTLCommandBuffer)throws{cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"stage context command failed: \(String(describing:cb.error))");gpuSeconds+=cb.gpuEndTime-cb.gpuStartTime;try flush()}
 // Runs after actual effects, inside their existing command buffer. Only
 // changed-page counts cross to the host at the already-required status read.
 func encodeRowChanges(_ cb:MTLCommandBuffer,_ rows:MTLBuffer,_ meta:MTLBuffer){
  let z=cb.makeBlitCommandEncoder()!;z.fill(buffer:pageCount,range:0..<4,value:0);z.fill(buffer:rowCount,range:0..<4,value:0);z.fill(buffer:journal,range:0..<4,value:0);z.endEncoding()
  scLaunch(cb,14,[meta,pageDispatch],1)
  let select=cb.makeComputeCommandEncoder()!;select.label="published row pages";select.setComputePipelineState(scPipelines[16]);for(i,b) in [rows,meta,pageBlocks,journal].enumerated(){select.setBuffer(b,offset:0,index:i)};let bounds=[UInt32(current.parts["rows"]!.words),current.metadata[2]];bounds.withUnsafeBytes{select.setBytes($0.baseAddress!,length:8,index:4)};select.dispatchThreadgroups(indirectBuffer:pageDispatch,indirectBufferOffset:0,threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));select.endEncoding()
  scLaunch(cb,17,[journal,pageDispatch],1)
  let e=cb.makeComputeCommandEncoder()!;e.label="commit journal to stage patch";e.setComputePipelineState(scPipelines[12])
  for (i,b) in [rows,rowPool.data,current.parts["rows"]!.rowPages!.map,pageBlocks,rowCount,pageCount].enumerated(){e.setBuffer(b,offset:0,index:i)}
  let cfg=[UInt32(current.parts["rows"]!.words),0,current.metadata[2],0];cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:16,index:6)};e.setBuffer(meta,offset:0,index:7);e.setBuffer(journal,offset:0,index:8)
  e.dispatchThreadgroups(indirectBuffer:pageDispatch,indirectBufferOffset:0,threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));e.endEncoding()
 }
 func flush()throws{
  guard let seal=pending else{return};try need(seal.command.status == .completed,"stage version observed before its seal completed")
  pending=nil;gpuSeconds+=seal.command.gpuEndTime-seal.command.gpuStartTime
  try need(rowPool.control.contents().advanced(by:4).load(as:UInt32.self)==0,"stage page pool exhausted")
  try need(seal.error.contents().load(as:UInt32.self)==0,"stage patch content mismatch")
  let n=Array(UnsafeBufferPointer(start:seal.counts.contents().bindMemory(to:UInt32.self,capacity:seal.names.count),count:seal.names.count))
  var counts=Dictionary(uniqueKeysWithValues:seal.names.enumerated().map{($0.element,Int(n[$0.offset]))}),changes=seal.changes
  counts["rows"]=seal.rowWords
  for (j,name) in seal.names.enumerated() where name != "rows"{let d=changes[name]!;changes[name]=SCChange(buffer:d.buffer,count:Int(n[j]),extent:d.extent)}
  if scExport{let a=try readWords(seal.before.parts["rows"]!),b=try readWords(seal.after.parts["rows"]!);var pairs:[UInt32]=[];for i in 0..<b.count where i>=a.count || a[i] != b[i]{pairs += [UInt32(i),b[i]]};try need(pairs.count/2==seal.rowWords,"row journal missed an update");changes["rows"]=SCChange(buffer:shared(pairs),count:pairs.count/2,extent:b.count)}
  patches[seal.phase]=SCPatch(from:seal.before.version,to:seal.after.version,changes:changes,header:seal.header)
  patchWords+=counts.values.reduce(0,+)
  history.append(["phase":seal.phase,"input_version":seal.before.version,"output_version":seal.after.version,"entry_epoch":seal.before.metadata[2],"exit_epoch":seal.after.metadata[2],"changed_words":counts,"entry_rows":seal.before.metadata[0],"exit_rows":seal.after.metadata[0]])
  account()
 }
 func seal(_ phase:Int,_ live:[String:SCPart],_ metadata:[UInt32],_ listCapacity:Int)throws{
  try need(pending==nil,"unconsumed stage seal")
  let before=current,version=try scNext(),names=live.keys.sorted(),cb=queue.makeCommandBuffer()!,header=shared([before.version,version]),error=shared([UInt32(0)])
  let zero=cb.makeBlitCommandEncoder()!;for b in counters{zero.fill(buffer:b,range:0..<4,value:0)};zero.endEncoding()
  var out:[String:SCPart]=[:],changed:[String:SCChange]=[:]
  for (j,name) in names.enumerated(){let a=before.parts[name]!,b=live[name]!
   if name=="rows"{let next=rowPool.snapshot(b,a,pageBlocks,Int(pageCount.contents().load(as:UInt32.self)),cb);out[name]=next
    if scAudit{scLaunch(cb,11,[rowPool.data,next.rowPages!.map,b.buffer,error],b.words,[UInt32(b.words)])};continue}
   // These index lists are append-only in the certified joint ABI. Function
   // keys are immutable Expr IDs; updating a value does not rewrite its row map.
   let appendOnly=["cost_rows","candidate_rows","function_rows","choice_rows","mutable_rows"].contains(name)
   let sameFunctions=before.parts["function_rows"]!.words==live["function_rows"]!.words
   // Direct Cell instructions can change state without an original-rule match.
   let recipeActive=(program["cell_program"] as? [String:Any])?["enabled"] as? Bool ?? false
   let updateActive=(program["cell_program"] as? [String:Any])?["update_program"] is [String:Any]
   let noActions=phase<2 && metadata[5]==0 && !recipeActive && !updateActive
   let noComparisonChanges=phase==2 && metadata[7]==before.metadata[7] && rowCount.contents().load(as:UInt32.self)==0
   let unchanged=b.words==a.words && (appendOnly || (name=="function_map" && sameFunctions) || (["parents","fact_parents","root_state"].contains(name) && noActions) || (["fact_parents","root_state"].contains(name) && noComparisonChanges && !(program["block_merge"] as? Bool ?? false)))
   if unchanged{out[name]=a;changed[name]=SCChange(buffer:empty,count:0,extent:b.words);if scAudit{scLaunch(cb,3,[a.buffer,b.buffer,error],b.words,[UInt32(b.words)],offsets:[a.offset,b.offset,0])};continue}
   let dest=privateBuffer(b.words*4),delta=scExport ? privateBuffer(b.words*8):empty
   scLaunch(cb,13,[a.buffer,b.buffer,dest,delta,counters[j]],b.words,[UInt32(a.words),UInt32(b.words),scExport ? 1:0,0],offsets:[a.offset,b.offset,0,0,0])
   if scAudit{scLaunch(cb,3,[dest,b.buffer,error],b.words,[UInt32(b.words)],offsets:[0,b.offset,0])}
   out[name]=SCPart(buffer:dest,words:b.words);changed[name]=SCChange(buffer:delta,count:0,extent:b.words);copyBytes+=b.words*4
  }
  scLaunch(cb,2,[header,empty,empty,error],1,[before.version,version,0,0])
  let read=cb.makeBlitCommandEncoder()!;for (j,b) in counters.enumerated(){read.copy(from:b,sourceOffset:0,to:countReadback,destinationOffset:j*4,size:4)};read.endEncoding()
  current=SCView(version,metadata,out,listCapacity)
  pending=SCSeal(phase:phase,before:before,after:current,command:cb,header:header,error:error,changes:changed,names:names,counts:countReadback,rowWords:Int(rowCount.contents().load(as:UInt32.self)))
  cb.commit()
  // Diagnostics may wait to export; production consumes the same commands at
  // the following stage's normal status boundary, never at a new host fence.
  if scExport{cb.waitUntilCompleted();sealWaits+=1;try flush()}
  account()
 }
 func readWords(_ p:SCPart)throws->[UInt32]{if p.words==0{return []};if let rows=p.rowPages{let cb=queue.makeCommandBuffer()!,dst=shared([UInt32](repeating:0,count:p.words));scLaunch(cb,10,[rows.pool.data,rows.map,dst],p.words,[UInt32(p.words)]);try wait(cb);return Array(UnsafeBufferPointer(start:dst.contents().bindMemory(to:UInt32.self,capacity:p.words),count:p.words))};let cb=queue.makeCommandBuffer()!,b=cb.makeBlitCommandEncoder()!,dst=shared([UInt32](repeating:0,count:p.words));b.copy(from:p.buffer,sourceOffset:p.offset*4,to:dst,destinationOffset:0,size:p.words*4);b.endEncoding();try wait(cb);return Array(UnsafeBufferPointer(start:dst.contents().bindMemory(to:UInt32.self,capacity:p.words),count:p.words))}
 func finish(_ scope:Int,liveRows:MTLBuffer)throws{
  let views=[base]+banks.map{$0 ?? base},nt=(program["tables"] as! [[String:Any]]).count
  var headers:[UInt32]=[];for v in views{headers += [v.version,v.version,v.metadata[0],v.metadata[1],v.metadata[8],v.metadata[2],UInt32(v.listCapacity),0]}
  let hb=shared(headers),result=shared([UInt32](repeating:0,count:4*(nt+2))),cb=queue.makeCommandBuffer()!,e=cb.makeComputeCommandEncoder()!
  e.setComputePipelineState(scPipelines[4]);e.label="parallel immutable stage scans"
  for (i,v) in views.enumerated(){e.setBuffer(rowPool.data,offset:0,index:i);e.setBuffer(v.parts["rows"]!.rowPages!.map,offset:0,index:12+i);e.setBuffer(v.parts["parents"]!.buffer,offset:0,index:i+4)}
  e.setBuffer(hb,offset:0,index:8);e.setBuffer(result,offset:0,index:9);e.setBuffer(scReadMask,offset:0,index:11);let cfg:[UInt32]=[4,UInt32(nt),0,0];cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:16,index:10)}
  e.dispatchThreads(MTLSize(width:max(1,views.map{Int($0.metadata[0])}.max()!),height:4,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));e.endEncoding();if scAudit,let arena=interface["set_arena"]{scLaunch(cb,6,[arena.buffer,liveRows,result],arena.words/2,[UInt32(arena.words/2)])};try wait(cb)
  let counts=Array(UnsafeBufferPointer(start:result.contents().bindMemory(to:UInt32.self,capacity:4*(nt+2)),count:4*(nt+2)))
  for i in 0..<4{try need(counts[i*(nt+2)]==0,"parallel scan used a mismatched stage context")}
  var report:[String:Any]=["scope":scope,"patches":history.count,"versions_created":history.count+1,"patch_words":patchWords,"device_copy_bytes":copyBytes+rowPool.growthCopyBytes+Int(rowPool.control.contents().advanced(by:12).load(as:UInt32.self))*1024,"row_pages_copied":Int(rowPool.control.contents().advanced(by:12).load(as:UInt32.self)),"row_storage":"shared immutable 1024-byte pages","row_patch_source":"commit page journal","seal_host_waits":sealWaits,"gpu_seconds":gpuSeconds,"peak_resident_bytes":peakBytes,"audit":scAudit,"history":history,"scan_versions":views.map{$0.version},"scan_read_tables":[Array(Set(scStages.flatMap{$0["reads"] as! [String]})).sorted()]+scStages.map{$0["reads"] as! [String]},"scan_table_counts":(0..<4).map{Array(counts[($0*(nt+2)+2)..<(($0+1)*(nt+2))])}]
  if scExport{
   var snapshots:[[String:Any]]=[];for v in views{var parts:[String:[UInt32]]=[:];for (name,p) in v.parts{parts[name]=try readWords(p)};snapshots.append(["version":v.version,"meta":v.metadata,"list_capacity":v.listCapacity,"parts":parts])}
   var finalParts:[String:[UInt32]]=[:];for (name,p) in current.parts{finalParts[name]=try readWords(p)}
   let last=finalParts["parents"]!;func find(_ p:[UInt32],_ x:UInt32)->UInt32{var x=x;while p[Int(x)] != x{x=p[Int(x)]};return x}
   var queries:[UInt32]=[]
   for id in 0..<Int(base.metadata[1]){let root=find(last,UInt32(id));if root != id{for bank in 0..<4{queries += [UInt32(bank),views[bank].version,0,UInt32(id),root,UInt32(nt)]}}}
   let tables=program["tables"] as! [[String:Any]],data=finalParts["rows"]!
   for row in 0..<Int(current.metadata[0]){
    let at=row*stride*2,table=Int(data[at]),spec=tables[table],schema=spec["schema"] as! [[String:Any]]
    if spec["kind"] as? String == "function",schema.count==2,schema[0]["kind"] as? String == "eq",data[at+2]==0{
     let key=data[at+8];if key<base.metadata[1]{for bank in 0..<4{queries += [UInt32(bank),views[bank].version,1,UInt32(table),key,UInt32(nt)]}}
    }
   }
   queries += [0,UInt32.max,0,0,0,UInt32(nt)] // Deliberate stale handle must fail.
   let qr=shared([Int64](repeating:0,count:queries.count/6*4)),qb=shared(queries),probe=queue.makeCommandBuffer()!
   let buffers=views.map{_ in rowPool.data}+views.map{$0.parts["parents"]!.buffer}+[hb,qr,qb,empty]+views.map{$0.parts["rows"]!.rowPages!.map}
   let pe=probe.makeComputeCommandEncoder()!;pe.setComputePipelineState(scPipelines[5]);for (i,b) in buffers.enumerated() where i != 11{pe.setBuffer(b,offset:0,index:i)};var nq=UInt32(queries.count/6);pe.setBytes(&nq,length:4,index:11);pe.dispatchThreads(MTLSize(width:Int(nq),height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));pe.endEncoding();try wait(probe)
   let answers=Array(UnsafeBufferPointer(start:qr.contents().bindMemory(to:Int64.self,capacity:queries.count/6*4),count:queries.count/6*4));try need(answers[answers.count-4]==1,"stale stage handle was accepted")
   var exportedPatches:[[String:Any]]=[]
   for (stage,patch) in patches.enumerated(){guard let patch=patch else{continue};var components:[String:Any]=[:];for (name,d) in patch.changes{components[name]=["extent":d.extent,"offset_values":try readWords(SCPart(buffer:d.buffer,words:d.count*2))]};exportedPatches.append(["stage":stage,"from":patch.from,"to":patch.to,"components":components])}
   let folder=URL(fileURLWithPath:CommandLine.arguments[1]).deletingLastPathComponent().appendingPathComponent("stage-contexts");try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
   var sharedParts:[String:[UInt32]]=[:];for (name,p) in interface{sharedParts[name]=try readWords(p)}
   let detail:[String:Any]=["shared_immutable":sharedParts,"schema":"stage-context-debug/v1","views":snapshots,"result":["version":current.version,"meta":current.metadata,"list_capacity":current.listCapacity,"parts":finalParts],"patches":exportedPatches,"queries":queries,"answers":answers,"scan_table_counts":report["scan_table_counts"]!,"scan_read_tables":report["scan_read_tables"]!]
   try JSONSerialization.data(withJSONObject:detail,options:[.sortedKeys]).write(to:folder.appendingPathComponent("scope-\(scope).json"))
   report["debug_export"]=true;report["gpu_seconds"]=gpuSeconds
  }
  if scAudit{
   // Deliberately applying a patch from another version must write nothing.
   let header=shared([base.version,current.version]),delta=shared([UInt32(0),UInt32(42)]),target=shared([UInt32(7),UInt32(9)]),error=shared([UInt32(0)]),cb=queue.makeCommandBuffer()!
   scLaunch(cb,2,[header,delta,target,error],1,[UInt32.max,current.version,2,1]);try wait(cb)
   try need(error.contents().load(as:UInt32.self)==1 && target.contents().load(as:UInt32.self)==7 && target.contents().advanced(by:4).load(as:UInt32.self)==9,"wrong-base patch changed a context")
   report["wrong_base_patch_rejected"]=true;report["gpu_seconds"]=gpuSeconds
  }
  scReports.append(report)
 }
}
func scWriteReport()throws{
 if !scEnabled{return};let result:[String:Any]=["schema":"stage-context-report/v1","scope_count":scReports.count,"patches":scReports.reduce(0){$0+($1["patches"] as! Int)},"audit":scAudit,"export":scExport,"scopes":scReports]
 let path=URL(fileURLWithPath:CommandLine.arguments[1]).deletingLastPathComponent().appendingPathComponent("stage-contexts.json")
 try JSONSerialization.data(withJSONObject:result,options:[.prettyPrinted,.sortedKeys]).write(to:path)
}
