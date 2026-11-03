// Compiled alongside runtime.swift. Coordinates, responses and score versions
// remain on Metal. Consumer scopes are exact scheduling metadata, not proofs.
func residentLoop() throws {
 let lowered=lib["resident_matrix"] as? [String:Any]
 try require(lowered?["schema"] as? String == "resident-ridge-response/v1","lower model before resident execution")
 let bank=lowered!,t=bank["token_count"] as! Int,v=bank["vocabulary_count"] as! Int,m=models.count
 let sorts=bank["sorts"] as! [String],modelSort=bank["model_sorts"] as! [UInt32]
 let constant=bank["constant_models"] as! [Bool],coeff=bank["weights"] as! [UInt16]
 try require(t==v+sorts.count && t<=2048 && modelSort.count==m && constant.count==m && coeff.count==t*m*3,"resident model dimensions")
 let scopeMode=ProcessInfo.processInfo.environment["STRUCTURAL_RESIDENT_SCOPE"] ?? "sort"
 try require(["consumers","sort"].contains(scopeMode),"resident scope")
 let weights=buffer(coeff),modelSortBuffer=buffer(modelSort),constants=buffer(constant.map{UInt32($0 ? 1:0)})
 let patch=try device.makeComputePipelineState(function:code.makeFunction(name:"resident_patch")!)
 let gemm=try device.makeComputePipelineState(function:code.makeFunction(name:"resident_response_mma")!)
 let normalize=try device.makeComputePipelineState(function:code.makeFunction(name:"resident_normalize")!)
 let gather=try device.makeComputePipelineState(function:code.makeFunction(name:"resident_gather")!)
 var capacity=0,versions:[UInt32]=[],sortIDs:[Int]=[],used:[Set<Int>]=[],computed:[Set<Int>]=[]
 var z:MTLBuffer?,meta:MTLBuffer?,raw:MTLBuffer?,scores:MTLBuffer?,scoreVersions:MTLBuffer?
 func grow(_ old:MTLBuffer?,_ bytes:Int,_ cb:MTLCommandBuffer)->MTLBuffer {
  let next=device.makeBuffer(length:max(8,bytes),options:.storageModePrivate)!
  let blit=cb.makeBlitCommandEncoder()!;blit.fill(buffer:next,range:0..<next.length,value:0)
  if let old=old{blit.copy(from:old,sourceOffset:0,to:next,destinationOffset:0,size:old.length)}
  blit.endEncoding();return next
 }
 try output.write(contentsOf:Data("RDC2".utf8))
 while true {
  let head=try read(4);let size=head.withUnsafeBytes{$0.loadUnaligned(as:UInt32.self)};if size==0{break}
  let data=try read(Int(size));try require(data.count%4==0,"resident packet alignment")
  let words=data.withUnsafeBytes{Array($0.bindMemory(to:UInt32.self))};var pos=0
  func word()throws->UInt32{try require(pos<words.count,"truncated resident packet");defer{pos+=1};return words[pos]}
  let n=Int(try word()),total=Int(try word()),pn=Int(try word()),oldCount=versions.count
  try require(total>=oldCount && total<Int(UInt32.max)/max(1,t*2,m*12) && pn<=total,"resident capacity")
  versions+=Array(repeating:0,count:total-oldCount);sortIDs+=Array(repeating:-1,count:total-oldCount)
  used+=Array(repeating:[],count:total-oldCount);computed+=Array(repeating:[],count:total-oldCount)
  var patches:[UInt32]=[],edits:[UInt32]=[],touched=Set<Int>(),discarded=0
  for _ in 0..<pn {
   let row=Int(try word()),version=try word(),s=Int(try word()),count=Int(try word())
   try require(row<total && s<sorts.count && versions[row]<UInt32.max && version==versions[row]+1 && (sortIDs[row]<0 || sortIDs[row]==s) && touched.insert(row).inserted,"invalid context update/version")
   discarded+=computed[row].subtracting(used[row]).filter{!constant[$0]}.count
   patches += [UInt32(row),version,UInt32(s),UInt32(edits.count/2),UInt32(count)]
   var changed=Set<UInt32>(),hasSort=versions[row]>0
   for _ in 0..<count {
    let token=try word(),on=try word()
    try require(token<t && on<=1 && changed.insert(token).inserted && (token<v || (token==v+s && on==1)),"invalid feature assignment")
    if token==v+s{hasSort=true};edits += [token,on]
   }
   try require(hasSort,"missing sort token")
   versions[row]=version;sortIDs[row]=s;used[row]=[];computed[row]=[]
  }
  try require(versions.allSatisfy{$0>0},"holes in resident context allocation")
  var planned:[Int:Set<Int>]=[:]
  let sn=Int(try word());try require(sn<=total,"too many context scopes")
  for _ in 0..<sn {
   let row=Int(try word()),count=Int(try word());try require(row<total && count<=m && planned[row]==nil,"invalid consumer scope")
   var scope=Set<Int>()
   for _ in 0..<count {
    let h=Int(try word());try require(h<m && Int(modelSort[h])==sortIDs[row] && scope.insert(h).inserted,"mistyped/duplicate consumer head")
   }
   planned[row]=scopeMode=="sort" ? Set(models.indices.filter{Int(modelSort[$0])==sortIDs[row]}) : scope
  }
  var requests:[UInt32]=[],unique=Set<Pair>(),cacheHits=0,prefetch=0,multi=0
  for _ in 0..<n {
   let row=Int(try word()),model=Int(try word()),version=try word()
   try require(row<total && model<m && version==versions[row] && Int(modelSort[model])==sortIDs[row],"stale or mistyped score request")
   requests += [UInt32(row),UInt32(model),version]
   // A late direct request is always admitted. Scopes never suppress matches.
   if scopeMode=="sort"{planned[row]=Set(models.indices.filter{Int(modelSort[$0])==sortIDs[row]})}
   else{planned[row,default:[]].insert(model)}
   let pair=Pair(row:row,column:model)
   if unique.insert(pair).inserted {
    if computed[row].contains(model){cacheHits+=1;if !used[row].contains(model) && !constant[model]{prefetch+=1}}
    if !constant[model]{used[row].insert(model)}
    if planned[row]!.filter({!constant[$0]}).count>1{multi+=1}
   }
  }
  try require(pos==words.count,"trailing resident packet")
  // Compare three complete layouts by actual 8x8 MMA tile count. Consumer
  // masks restrict useful work, but gratuitous fragmentation wastes padding.
  // Tie break by arithmetic cells, then bank count; no learned threshold.
  var demand:[Int:[Int]]=[:],pairs:[UInt32]=[],blocks:[Block]=[]
  for row in planned.keys.sorted() {
   let missing=planned[row]!.subtracting(computed[row]).sorted()
   pairs += missing.filter{constant[$0]}.flatMap{[UInt32(row),UInt32($0)]}
   computed[row].formUnion(missing.filter{constant[$0]})
   let hs=missing.filter{!constant[$0]};if !hs.isEmpty{demand[row]=hs}
  }
  for s in sorts.indices {
   let rs=demand.keys.filter{sortIDs[$0]==s}.sorted();if rs.isEmpty{continue}
   let hs=Set(rs.flatMap{demand[$0]!}).sorted()
   if scopeMode=="sort"{blocks.append(Block(rows:rs,columns:hs.flatMap{[$0*3,$0*3+1,$0*3+2]}));continue}
   var identical:[[Int]:[Int]]=[:]
   for row in rs{identical[demand[row]!,default:[]].append(row)}
   func cols(_ h:[Int])->[Int]{h.flatMap{[$0*3,$0*3+1,$0*3+2]}}
   let tight=identical.keys.sorted(by:{$0.lexicographicallyPrecedes($1)}).map{Block(rows:identical[$0]!,columns:cols($0))}
   let separate=hs.map{h in Block(rows:rs.filter{demand[$0]!.contains(h)},columns:cols([h]))}
   let rectangle=[Block(rows:rs,columns:cols(hs))]
   func cost(_ bs:[Block])->[Int]{[bs.reduce(0){$0+$1.tiles},bs.reduce(0){$0+$1.cells},bs.count]}
   blocks += [tight,separate,rectangle].min{cost($0).lexicographicallyPrecedes(cost($1))}!
  }
  var desc:[UInt32]=[],rowIDs:[UInt32]=[],colIDs:[UInt32]=[],tiles=0,cells=0,computedHeads=0,maxRows=0,maxCols=0
  for block in blocks {
   let rows=block.rows,cols=block.columns,hs=stride(from:0,to:cols.count,by:3).map{cols[$0]/3}
   let firstRow=rowIDs.count,firstCol=colIDs.count;rowIDs+=rows.map(UInt32.init);colIDs+=cols.map(UInt32.init)
   tiles+=block.tiles
   desc += [rows.count,cols.count,firstRow,firstCol,0,tiles,m*3].map(UInt32.init)
   cells+=block.cells;computedHeads+=rows.count*hs.count;maxRows=max(maxRows,rows.count);maxCols=max(maxCols,cols.count)
   for row in rows{pairs+=hs.flatMap{[UInt32(row),UInt32($0)]};computed[row].formUnion(hs)}
  }
  let banks=blocks.count,coordinateRows=demand.count
  let command=queue.makeCommandBuffer()!
  if total>capacity {
   var next=max(1,capacity);while next<total{next*=2}
   z=grow(z,next*t*2,command);meta=grow(meta,next*3*4,command);raw=grow(raw,next*m*3*4,command);scores=grow(scores,next*m*4,command);scoreVersions=grow(scoreVersions,next*m*4,command);capacity=next
  }
  let error=buffer([UInt32(0)]),answer=device.makeBuffer(length:max(8,n*4),options:.storageModeShared)!
  if pn>0{launch(command,patch,[z!,meta!,buffer(patches),buffer(edits),error],[UInt32(pn),UInt32(t)],pn)}
  if banks>0{launch(command,gemm,[buffer(desc),z!,weights,buffer(rowIDs),buffer(colIDs),raw!],[UInt32(banks),UInt32(t),UInt32(m*3),0],tiles,true)}
  if !pairs.isEmpty{launch(command,normalize,[buffer(pairs),meta!,modelSortBuffer,constants,raw!,bb,scores!,scoreVersions!,error],[UInt32(pairs.count/2),UInt32(m)],pairs.count/2)}
  if n>0{launch(command,gather,[buffer(requests),meta!,modelSortBuffer,scores!,scoreVersions!,answer,error],[UInt32(n),UInt32(m)],n)}
  command.commit();command.waitUntilCompleted();try require(command.status == .completed,"resident GPU command failed")
  try require(error.contents().load(as:UInt32.self)==0,"resident GPU version/count check failed")
  var response=Data();var count=UInt32(n),duration=command.gpuEndTime-command.gpuStartTime
  withUnsafeBytes(of:&count){response.append(contentsOf:$0)};withUnsafeBytes(of:&duration){response.append(contentsOf:$0)}
  let metrics=[n,coordinateRows,unique.count,cells,tiles*64,banks,multi,maxCols,maxRows,0].map(UInt64.init)
  let unused=versions.indices.reduce(0){$0+computed[$1].subtracting(used[$1]).filter{!constant[$0]}.count}
  let resident=[total,pn,edits.count/2,cacheHits,prefetch,computedHeads,unused,discarded,(z?.length ?? 0)+(meta?.length ?? 0)+(raw?.length ?? 0)+(scores?.length ?? 0)+(scoreVersions?.length ?? 0)].map(UInt64.init)
  metrics.withUnsafeBytes{response.append(contentsOf:$0)};resident.withUnsafeBytes{response.append(contentsOf:$0)}
  response.append(Data(bytes:answer.contents(),count:n*4));try output.write(contentsOf:response)
 }
}
