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
let scNames=["sc_count","sc_pack","sc_apply","sc_equal","sc_scan","sc_probe","sc_arena_audit"]
let scPipelines=try (scEnabled ? scNames:[]).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
var scVersion:UInt32=0
var scReports:[[String:Any]]=[]
struct SCPart {let buffer:MTLBuffer;let words:Int;let offset:Int;init(buffer:MTLBuffer,words:Int,offset:Int=0){self.buffer=buffer;self.words=words;self.offset=offset}}
final class SCView {
 let version:UInt32;let metadata:[UInt32];let parts:[String:SCPart];let listCapacity:Int
 init(_ version:UInt32,_ metadata:[UInt32],_ parts:[String:SCPart],_ listCapacity:Int){self.version=version;self.metadata=metadata;self.parts=parts;self.listCapacity=listCapacity}
 var bytes:Int{parts.values.map{$0.buffer.length}.reduce(0,+)}
}
struct SCChange {let buffer:MTLBuffer;let count:Int;let extent:Int}
struct SCPatch {let from:UInt32;let to:UInt32;let changes:[String:SCChange];let header:MTLBuffer;var bytes:Int{changes.values.map{$0.buffer.length}.reduce(0,+)+header.length}}
func scNext()throws->UInt32{try need(scVersion<UInt32.max,"stage context version exhausted");scVersion+=1;return scVersion}
func scLaunch(_ cb:MTLCommandBuffer,_ kernel:Int,_ buffers:[MTLBuffer],_ n:Int,_ cfg:[UInt32]=[],offsets:[Int]=[]){
 if n==0{return};let e=cb.makeComputeCommandEncoder()!;e.label=scNames[kernel];e.setComputePipelineState(scPipelines[kernel])
 for (i,b) in buffers.enumerated(){e.setBuffer(b,offset:offsets.isEmpty ? 0:offsets[i]*4,index:i)}
 if !cfg.isEmpty{cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:buffers.count)}}
 let threads=kernel<2 ? ((n+255)/256)*256:n
 e.dispatchThreads(MTLSize(width:threads,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));e.endEncoding()
}
final class StageContexts {
 let queue:MTLCommandQueue
 let base:SCView;var current:SCView;var banks=[SCView?](repeating:nil,count:3);var patches=[SCPatch?](repeating:nil,count:3)
 let interface:[String:SCPart];let counters:[MTLBuffer];let countReadback:MTLBuffer;let empty:MTLBuffer;var blockCounts:[MTLBuffer]
 var history:[[String:Any]]=[];var gpuSeconds=0.0;var copyBytes=0;var patchWords=0;var peakBytes=0
 init(_ live:[String:SCPart],_ metadata:[UInt32],_ listCapacity:Int,_ interface:[String:SCPart],queue:MTLCommandQueue)throws{
  self.queue=queue;counters=live.keys.map{_ in privateBuffer(4)};countReadback=shared([UInt32](repeating:0,count:live.count));empty=privateBuffer(8);blockCounts=live.keys.map{_ in privateBuffer(8)}
  let cb=queue.makeCommandBuffer()!,b=cb.makeBlitCommandEncoder()!;var parts:[String:SCPart]=[:],frozen:[String:SCPart]=[:]
  for (name,p) in live{let dst=privateBuffer(p.words*4);if p.words>0{b.copy(from:p.buffer,sourceOffset:p.offset*4,to:dst,destinationOffset:0,size:p.words*4)};parts[name]=SCPart(buffer:dst,words:p.words);copyBytes+=p.words*4}
  for (name,p) in interface{let dst=privateBuffer(p.words*4);if p.words>0{b.copy(from:p.buffer,sourceOffset:p.offset*4,to:dst,destinationOffset:0,size:p.words*4)};frozen[name]=SCPart(buffer:dst,words:p.words);copyBytes+=p.words*4}
  b.endEncoding();cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"stage entry snapshot failed")
  base=SCView(try scNext(),metadata,parts,listCapacity);current=base;self.interface=frozen;gpuSeconds=cb.gpuEndTime-cb.gpuStartTime;account()
 }
 func account(){var seen=Set<ObjectIdentifier>();var buffers=([base,current]+banks.compactMap{$0}).flatMap{$0.parts.values.map{$0.buffer}}+interface.values.map{$0.buffer}+counters+blockCounts+[countReadback,empty]
  for p in patches.compactMap({$0}){buffers += p.changes.values.map{$0.buffer}+[p.header]}
  peakBytes=max(peakBytes,buffers.filter{seen.insert(ObjectIdentifier($0)).inserted}.map{$0.length}.reduce(0,+))}
 func begin(_ phase:Int)->SCView{banks[phase]=current;account();return current}
 func wait(_ cb:MTLCommandBuffer)throws{cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"stage context command failed: \(String(describing:cb.error))");gpuSeconds+=cb.gpuEndTime-cb.gpuStartTime}
 func seal(_ phase:Int,_ live:[String:SCPart],_ metadata:[UInt32],_ listCapacity:Int)throws{
  try autoreleasepool {
   let before=current,version=try scNext(),names=live.keys.sorted();let count=countReadback
   func domain(_ name:String)->(MTLBuffer,Int,[UInt32]){
    let a=before.parts[name]!,b=live[name]!
    if name != "rows"{return (empty,b.words,[UInt32(a.words),UInt32(b.words),0,0])}
    let kind=scStages[phase]["row_write_domain"] as! String
    let index=kind=="function_rows_and_append" ? before.parts["function_rows"]:kind=="mutable_rows" ? before.parts["mutable_rows"]:nil
    let count=index?.words ?? 0
    return (index?.buffer ?? empty,count*stride*2+max(0,b.words-a.words),[UInt32(a.words),UInt32(b.words),1,UInt32(count)])
   }
   let counts=counters;let scan=queue.makeCommandBuffer()!,zero=scan.makeBlitCommandEncoder()!
   for b in counts{zero.fill(buffer:b,range:0..<4,value:0)};zero.endEncoding()
   for (j,name) in names.enumerated(){let a=before.parts[name]!,b=live[name]!;try need(max(a.words,b.words)<Int(UInt32.max),"stage component address space exhausted")
    let (indices,work,cfg)=domain(name);let bytes=((work+255)/256)*4;if blockCounts[j].length<bytes{blockCounts[j]=privateBuffer(bytes)}
    scLaunch(scan,0,[a.buffer,b.buffer,counts[j],blockCounts[j],indices],work,cfg,offsets:[a.offset,b.offset,0,0,0])}
   let read=scan.makeBlitCommandEncoder()!;for (j,b) in counts.enumerated(){read.copy(from:b,sourceOffset:0,to:count,destinationOffset:j*4,size:4)};read.endEncoding();try wait(scan)
   let n=Array(UnsafeBufferPointer(start:count.contents().bindMemory(to:UInt32.self,capacity:names.count),count:names.count))
   let cb=queue.makeCommandBuffer()!,clear=cb.makeBlitCommandEncoder()!;for b in counts{clear.fill(buffer:b,range:0..<4,value:0)};clear.endEncoding()
   let header=shared([before.version,version]),error=shared([UInt32(0)]);var changed:[String:SCChange]=[:],out:[String:SCPart]=[:]
   for (j,name) in names.enumerated(){let a=before.parts[name]!,b=live[name]!,delta=n[j]==0 ? empty:privateBuffer(Int(n[j])*8)
    if n[j]==0{try need(b.words<=a.words,"growing component has no patch");if scAudit{scLaunch(cb,3,[a.buffer,b.buffer,error],b.words,[UInt32(b.words)],offsets:[a.offset,b.offset,0])};changed[name]=SCChange(buffer:delta,count:0,extent:b.words);out[name]=SCPart(buffer:a.buffer,words:b.words);continue}
    let dest=privateBuffer(b.words*4)
    // The destination is derived from its parent, never copied from live.
    let copy=cb.makeBlitCommandEncoder()!;let bytes=min(a.words,b.words)*4;if bytes>0{copy.copy(from:a.buffer,sourceOffset:0,to:dest,destinationOffset:0,size:bytes)};copy.endEncoding();copyBytes+=bytes
    let (indices,work,cfg)=domain(name);scLaunch(cb,1,[a.buffer,b.buffer,delta,counts[j],blockCounts[j],indices],work,cfg,offsets:[a.offset,b.offset,0,0,0,0])
    scLaunch(cb,2,[header,delta,dest,error],max(1,Int(n[j])),[before.version,version,UInt32(b.words),n[j]])
    if scAudit{scLaunch(cb,3,[dest,b.buffer,error],b.words,[UInt32(b.words)],offsets:[0,b.offset,0])}
    changed[name]=SCChange(buffer:delta,count:Int(n[j]),extent:b.words);out[name]=SCPart(buffer:dest,words:b.words);patchWords+=Int(n[j])
   }
   scLaunch(cb,2,[header,empty,empty,error],1,[before.version,version,0,0])
   try wait(cb);try need(error.contents().load(as:UInt32.self)==0,"stage patch base/version/content mismatch")
   let patch=SCPatch(from:before.version,to:version,changes:changed,header:header)
   patches[phase]=patch;current=SCView(version,metadata,out,listCapacity)
   history.append(["phase":phase,"input_version":before.version,"output_version":version,"entry_epoch":before.metadata[2],"exit_epoch":metadata[2],"changed_words":Dictionary(uniqueKeysWithValues:names.enumerated().map{($0.element,Int(n[$0.offset]))}),"entry_rows":before.metadata[0],"exit_rows":metadata[0]])
   account()
  }
 }
 func readWords(_ p:SCPart)throws->[UInt32]{if p.words==0{return []};let cb=queue.makeCommandBuffer()!,b=cb.makeBlitCommandEncoder()!,dst=shared([UInt32](repeating:0,count:p.words));b.copy(from:p.buffer,sourceOffset:p.offset*4,to:dst,destinationOffset:0,size:p.words*4);b.endEncoding();try wait(cb);return Array(UnsafeBufferPointer(start:dst.contents().bindMemory(to:UInt32.self,capacity:p.words),count:p.words))}
 func finish(_ scope:Int,liveRows:MTLBuffer)throws{
  let views=[base]+banks.map{$0 ?? base},nt=(program["tables"] as! [[String:Any]]).count
  var headers:[UInt32]=[];for v in views{headers += [v.version,v.version,v.metadata[0],v.metadata[1],v.metadata[8],v.metadata[2],UInt32(v.listCapacity),0]}
  let hb=shared(headers),result=shared([UInt32](repeating:0,count:4*(nt+2))),cb=queue.makeCommandBuffer()!,e=cb.makeComputeCommandEncoder()!
  e.setComputePipelineState(scPipelines[4]);e.label="parallel immutable stage scans"
  for (i,v) in views.enumerated(){e.setBuffer(v.parts["rows"]!.buffer,offset:0,index:i);e.setBuffer(v.parts["parents"]!.buffer,offset:0,index:i+4)}
  e.setBuffer(hb,offset:0,index:8);e.setBuffer(result,offset:0,index:9);e.setBuffer(scReadMask,offset:0,index:11);let cfg:[UInt32]=[4,UInt32(nt),0,0];cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:16,index:10)}
  e.dispatchThreads(MTLSize(width:max(1,views.map{Int($0.metadata[0])}.max()!),height:4,depth:1),threadsPerThreadgroup:MTLSize(width:256,height:1,depth:1));e.endEncoding();if scAudit,let arena=interface["set_arena"]{scLaunch(cb,6,[arena.buffer,liveRows,result],arena.words/2,[UInt32(arena.words/2)])};try wait(cb)
  let counts=Array(UnsafeBufferPointer(start:result.contents().bindMemory(to:UInt32.self,capacity:4*(nt+2)),count:4*(nt+2)))
  for i in 0..<4{try need(counts[i*(nt+2)]==0,"parallel scan used a mismatched stage context")}
  var report:[String:Any]=["scope":scope,"patches":history.count,"versions_created":history.count+1,"patch_words":patchWords,"device_copy_bytes":copyBytes,"gpu_seconds":gpuSeconds,"peak_resident_bytes":peakBytes,"audit":scAudit,"history":history,"scan_versions":views.map{$0.version},"scan_read_tables":[Array(Set(scStages.flatMap{$0["reads"] as! [String]})).sorted()]+scStages.map{$0["reads"] as! [String]},"scan_table_counts":(0..<4).map{Array(counts[($0*(nt+2)+2)..<(($0+1)*(nt+2))])}]
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
   let buffers=views.map{$0.parts["rows"]!.buffer}+views.map{$0.parts["parents"]!.buffer}+[hb,qr,qb]
   scLaunch(probe,5,buffers,queries.count/6,[UInt32(queries.count/6)]);try wait(probe)
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
