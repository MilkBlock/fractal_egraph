// Compiled alongside runtime.swift. The model, context coordinates, intermediate
// responses and prediction cache stay on Metal. Host metadata validates the ABI
// and reports speculative work; it does not contain exact predicate answers.
func residentLoop() throws {
 let lowered=lib["resident_matrix"] as? [String:Any]
 try require(lowered?["schema"] as? String == "resident-ridge-response/v1","lower model before resident execution")
 let bank=lowered!,t=bank["token_count"] as! Int,v=bank["vocabulary_count"] as! Int,m=models.count
 let sorts=bank["sorts"] as! [String],modelSort=bank["model_sorts"] as! [UInt32]
 let constant=bank["constant_models"] as! [Bool],coeff=bank["weights"] as! [UInt16]
 try require(t==v+sorts.count && t<=2048 && modelSort.count==m && constant.count==m && coeff.count==t*m*3,"resident model dimensions")
 let heads=sorts.indices.map{s in models.indices.filter{modelSort[$0]==s && !constant[$0]}}
 let weights=buffer(coeff),modelSortBuffer=buffer(modelSort),constants=buffer(constant.map{UInt32($0 ? 1:0)})
 let patch=try device.makeComputePipelineState(function:code.makeFunction(name:"resident_patch")!)
 let gemm=try device.makeComputePipelineState(function:code.makeFunction(name:"resident_response_mma")!)
 let normalize=try device.makeComputePipelineState(function:code.makeFunction(name:"resident_normalize")!)
 let gather=try device.makeComputePipelineState(function:code.makeFunction(name:"resident_gather")!)
 var capacity=0,versions:[UInt32]=[],sortIDs:[Int]=[],used:[Set<Int>]=[]
 var z:MTLBuffer?,meta:MTLBuffer?,raw:MTLBuffer?,scores:MTLBuffer?
 func grow(_ old:MTLBuffer?,_ bytes:Int,_ cb:MTLCommandBuffer)->MTLBuffer {
  let next=device.makeBuffer(length:max(8,bytes),options:.storageModePrivate)!
  let blit=cb.makeBlitCommandEncoder()!;blit.fill(buffer:next,range:0..<next.length,value:0)
  if let old=old{blit.copy(from:old,sourceOffset:0,to:next,destinationOffset:0,size:old.length)}
  blit.endEncoding();return next
 }
 try output.write(contentsOf:Data("RDC1".utf8))
 while true {
  let head=try read(4);let size=head.withUnsafeBytes{$0.loadUnaligned(as:UInt32.self)};if size==0{break}
  let data=try read(Int(size));try require(data.count%4==0,"resident packet alignment")
  let words=data.withUnsafeBytes{Array($0.bindMemory(to:UInt32.self))};var pos=0
  func word()throws->UInt32{try require(pos<words.count,"truncated resident packet");defer{pos+=1};return words[pos]}
  let n=Int(try word()),total=Int(try word()),pn=Int(try word()),oldCount=versions.count
  try require(total>=oldCount && total<Int(UInt32.max)/max(1,t*2,m*12) && pn<=total,"resident capacity")
  versions+=Array(repeating:0,count:total-oldCount);sortIDs+=Array(repeating:-1,count:total-oldCount);used+=Array(repeating:[],count:total-oldCount)
  var patches:[UInt32]=[],edits:[UInt32]=[],dirty:[Int]=[],touched=Set<Int>(),discarded=0
  for _ in 0..<pn {
   let row=Int(try word()),version=try word(),s=Int(try word()),count=Int(try word())
   try require(row<total && s<sorts.count && versions[row]<UInt32.max && version==versions[row]+1 && (sortIDs[row]<0 || sortIDs[row]==s) && touched.insert(row).inserted,"invalid context update/version")
   if versions[row]>0{discarded+=heads[s].count-used[row].count}
   patches += [UInt32(row),version,UInt32(s),UInt32(edits.count/2),UInt32(count)]
   var changed=Set<UInt32>(),hasSort=versions[row]>0
   for _ in 0..<count {
    let token=try word(),on=try word()
    try require(token<t && on<=1 && changed.insert(token).inserted && (token<v || (token==v+s && on==1)),"invalid feature assignment")
    if token==v+s{hasSort=true};edits += [token,on]
   }
   try require(hasSort,"missing sort token")
   versions[row]=version;sortIDs[row]=s;used[row]=[];dirty.append(row)
  }
  try require(versions.allSatisfy{$0>0},"holes in resident context allocation")
  var requests:[UInt32]=[],unique=Set<Pair>(),cacheHits=0,prefetch=0,multi=0
  for _ in 0..<n {
   let row=Int(try word()),model=Int(try word()),version=try word()
   try require(row<total && model<m && version==versions[row] && Int(modelSort[model])==sortIDs[row],"stale or mistyped score request")
   requests += [UInt32(row),UInt32(model),version]
   let pair=Pair(row:row,column:model)
   if unique.insert(pair).inserted {
    if !touched.contains(row){cacheHits+=1;if !used[row].contains(model) && !constant[model]{prefetch+=1}}
    if !constant[model]{used[row].insert(model)}
    if heads[sortIDs[row]].count>1{multi+=1}
   }
  }
  try require(pos==words.count,"trailing resident packet")
  var desc:[UInt32]=[],rowIDs:[UInt32]=[],colIDs:[UInt32]=[],tiles=0,cells=0,computedHeads=0,banks=0,maxRows=0,maxCols=0
  for s in sorts.indices {
   let rows=dirty.filter{sortIDs[$0]==s},cols=heads[s].flatMap{[$0*3,$0*3+1,$0*3+2]}
   if rows.isEmpty || cols.isEmpty{continue}
   let firstRow=rowIDs.count,firstCol=colIDs.count;rowIDs+=rows.map(UInt32.init);colIDs+=cols.map(UInt32.init)
   tiles+=((rows.count+7)/8)*((cols.count+7)/8)
   desc += [rows.count,cols.count,firstRow,firstCol,0,tiles,m*3].map(UInt32.init)
   cells+=rows.count*cols.count;computedHeads+=rows.count*heads[s].count;banks+=1;maxRows=max(maxRows,rows.count);maxCols=max(maxCols,cols.count)
  }
  let command=queue.makeCommandBuffer()!
  if total>capacity {
   var next=max(1,capacity);while next<total{next*=2}
   z=grow(z,next*t*2,command);meta=grow(meta,next*3*4,command);raw=grow(raw,next*m*3*4,command);scores=grow(scores,next*m*4,command);capacity=next
  }
  let error=buffer([UInt32(0)]),answer=device.makeBuffer(length:max(8,n*4),options:.storageModeShared)!
  if pn>0 {
   launch(command,patch,[z!,meta!,buffer(patches),buffer(edits),error],[UInt32(pn),UInt32(t)],pn)
   if banks>0{launch(command,gemm,[buffer(desc),z!,weights,buffer(rowIDs),buffer(colIDs),raw!],[UInt32(banks),UInt32(t),UInt32(m*3),0],tiles,true)}
   launch(command,normalize,[buffer(dirty.map(UInt32.init)),meta!,modelSortBuffer,constants,raw!,bb,scores!,error],[UInt32(dirty.count),UInt32(m)],dirty.count)
  }
  if n>0{launch(command,gather,[buffer(requests),meta!,modelSortBuffer,scores!,answer,error],[UInt32(n),UInt32(m)],n)}
  command.commit();command.waitUntilCompleted();try require(command.status == .completed,"resident GPU command failed")
  try require(error.contents().load(as:UInt32.self)==0,"resident GPU version/count check failed")
  var response=Data();var count=UInt32(n),duration=command.gpuEndTime-command.gpuStartTime
  withUnsafeBytes(of:&count){response.append(contentsOf:$0)};withUnsafeBytes(of:&duration){response.append(contentsOf:$0)}
  let metrics=[n,dirty.count,unique.count,cells,tiles*64,banks,multi,maxCols,maxRows,0].map(UInt64.init)
  let unused=versions.indices.reduce(0){$0+heads[sortIDs[$1]].count-used[$1].count}
  let resident=[total,pn,edits.count/2,cacheHits,prefetch,computedHeads,unused,discarded,(z?.length ?? 0)+(meta?.length ?? 0)+(raw?.length ?? 0)+(scores?.length ?? 0)].map(UInt64.init)
  metrics.withUnsafeBytes{response.append(contentsOf:$0)};resident.withUnsafeBytes{response.append(contentsOf:$0)}
  response.append(Data(bytes:answer.contents(),count:n*4));try output.write(contentsOf:response)
 }
}
